//! 配网凭据存储后端（0.0.5 批M M-γ，MC-6：凭据持久化）。
//!
//! 分工铁律（工单 MC-6 原文）：**station 侧凭据由后端承载持久化**（linux =
//! NetworkManager 连接配置），**件内不自写文件**；[`crate::provision_store`]
//! 的消费方（rr3-connect 的 [`Rr3Provision`](模块侧配网状态机)）只管态不管存。
//! 件内不出现落盘路径字面量——持久化策略归 NM 本身（连接配置目录由 NM
//! daemon 管辖），本后端只经 nmcli 子进程下单，不碰任何路径。
//!
//! 连接名锚 [`RR3_NM_PROVISION_CON`]：SSID 是用户数据、连接名是本后端的
//! 稳定锚（在册查询/更新/删除都以它定位；换网 = 同名 profile 改 ssid/psk，
//! 不为每个 SSID 建连接——那会让"在册吗"变成全表扫）。
//!
//! nmcli 序列（配网是 runbook §5.5.2 件 4 既定口径，全部落在 nmcli 上）：
//! - 在册查询：`nmcli -t -f NAME con show` 逐行找锚名；
//! - connect：在册 → `con modify <锚> 802-11-wireless.ssid S …psk P …key-mgmt wpa-psk`；
//!   不在册 → `con add type wifi con-name <锚> 802-11-wireless.ssid S …`——
//!   add/modify 同属性路径，key-mgmt 显式 wpa-psk（connect 参数语义恒 WPA，
//!   密码 ≥8 位由模块侧 legacy 语义裁决，承载层不做策略重复校验）；
//! - forget：在册 → `con delete <锚>`；不在册 → 直接 Ok（未连先忘幂等回 ap
//!   是裁定语义，delete 一个不存在的 profile 不是错误）；
//! - 在册 SSID 读回：`nmcli -t -f 802-11-wireless.ssid con show <锚>`。
//!
//! psk 经 argv 传递：与 `nmcli dev wifi connect … password` 既定口径一致
//! （同机用户可经 ps 观测 argv 是 nmcli 全家共有的暴露面），passwd-file
//! 收敛留待立项时随 runbook 改口径，此处不另发明。
//!
//! 编译门 `cfg(any(target_os = "linux", test))`：linux 全编译；macOS test 面
//! 编译——"connect → 重启 → 仍在册"判据要求**主机可测**：假 nmcli 脚本以
//! `NMCONF_HOME` 为连接配置目录模拟 NM 的持久化语义（脚本自 export，测试
//! 进程零 set_var 竞态），同夹具下重新构造后端 = 重启后查在册。
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use crate::nmcli_support::{rr3_nm_field, rr3_nm_run, rr3_nm_unescape};
use crate::{Rr3HalError, Rr3SsidText};

/// 配网连接的固定 profile 名（本后端的在册锚，见模块 doc）
pub const RR3_NM_PROVISION_CON: &str = "rr3-provision";

/// 配网凭据存储面（MC-6）：三动作——connect 落盘、forget 真删、在册查询。
/// 全部由后端承载（linux = NM 连接配置）；实现方构造不触设备（选择函数
/// 口径），首次调用才起子进程。
pub trait Rr3WifiStore: Send + Sync {
    /// 存凭据进持久化承载（在册则更新，不在册则新建；调用方在成功后自行
    /// 转移配网态——失败时状态机不动，fail-closed）
    fn rr3_wifi_store_connect(&self, ssid: &str, pass: &str) -> Result<(), Rr3HalError>;
    /// 删除在册凭据（不在册 = 幂等 Ok；删除失败显式回错——"forget 后配置
    /// 真消失"要求失败不得静默装成功）
    fn rr3_wifi_store_forget(&self) -> Result<(), Rr3HalError>;
    /// 在册 SSID（None = 不在册）：重启后模块据此恢复配网态
    fn rr3_wifi_store_ssid(&self) -> Result<Option<Rr3SsidText>, Rr3HalError>;
}

/// linux NetworkManager 凭据存储（[`Rr3WifiStore`] 的 linux 后端，nmcli 子进程
/// 承载）。机制本体（spawn + 超时守护 + 管道读）与链路读数后端共用
/// [`crate::nmcli_support`] 一份真值。
pub struct Rr3NmWifiStore {
    nmcli: PathBuf,
    nmcli_timeout: Duration,
}

impl Rr3NmWifiStore {
    /// 指定 nmcli 程序路径与子进程超时构造（选择函数从模块 config 的
    /// `timeout_ms` 传入超时——与链路读数后端同一档，没有代码缺省；测试传
    /// 假脚本绝对路径注入编排，零全局环境污染）
    pub fn rr3_nm_wifi_store_new(
        nmcli: impl Into<PathBuf>,
        nmcli_timeout: Duration,
    ) -> Self {
        Self {
            nmcli: nmcli.into(),
            nmcli_timeout,
        }
    }

    /// 在册吗：`-t -f NAME con show` 逐行找锚名（NAME 值不含 terse 转义——
    /// 锚名是本后端常量，无冒号无反斜杠）
    fn in_register(&self) -> Result<bool, Rr3HalError> {
        let names = rr3_nm_run(
            &self.nmcli,
            &["-t", "-f", "NAME", "con", "show"],
            self.nmcli_timeout,
        )?;
        Ok(names
            .lines()
            .any(|l| l.trim() == RR3_NM_PROVISION_CON))
    }

    /// add/modify 公共属性段：ssid + psk + key-mgmt（add 与 modify 同形状，
    /// 假脚本与真 nmcli 同一份属性解析）。返回 owned 串——argv 引用面在
    /// 调用点收敛（&str 混 'static 字面量与调用方借用会拉出多生命周期）。
    fn upsert_args(&self, verb: &str, ssid: &str, pass: &str) -> Vec<String> {
        let mut args: Vec<String> = vec!["con".into(), verb.into()];
        if verb == "modify" {
            args.push(RR3_NM_PROVISION_CON.into());
        } else {
            args.extend(["type", "wifi", "con-name"].iter().map(|s| s.to_string()));
            args.push(RR3_NM_PROVISION_CON.into());
        }
        args.extend([
            "802-11-wireless.ssid",
            ssid,
            "802-11-wireless-security.psk",
            pass,
            "802-11-wireless-security.key-mgmt",
            "wpa-psk",
        ]
        .iter()
        .map(|s| s.to_string()));
        args
    }
}

impl Rr3WifiStore for Rr3NmWifiStore {
    fn rr3_wifi_store_connect(&self, ssid: &str, pass: &str) -> Result<(), Rr3HalError> {
        let verb = if self.in_register()? {
            "modify"
        } else {
            "add"
        };
        let args = self.upsert_args(verb, ssid, pass);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        rr3_nm_run(&self.nmcli, &refs, self.nmcli_timeout).map(|_| ())
    }

    fn rr3_wifi_store_forget(&self) -> Result<(), Rr3HalError> {
        if !self.in_register()? {
            return Ok(()); // 未连先忘幂等：不在册时连 delete 都不用发
        }
        rr3_nm_run(
            &self.nmcli,
            &["con", "delete", RR3_NM_PROVISION_CON],
            self.nmcli_timeout,
        )
        .map(|_| ())
    }

    fn rr3_wifi_store_ssid(&self) -> Result<Option<Rr3SsidText>, Rr3HalError> {
        if !self.in_register()? {
            return Ok(None);
        }
        let out = rr3_nm_run(
            &self.nmcli,
            &[
                "-t",
                "-f",
                "802-11-wireless.ssid",
                "con",
                "show",
                RR3_NM_PROVISION_CON,
            ],
            self.nmcli_timeout,
        )?;
        // terse 单字段行 `802-11-wireless.ssid:<值>`（字段提取与链路读数共用
        // nmcli_support 一份真值）；NM 空值 `--`/空串 → None
        let v = rr3_nm_field(&out, "802-11-wireless.ssid")
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != "--");
        Ok(v.map(|s| Rr3SsidText::rr3_ssid_text_new(&rr3_nm_unescape(s))))
    }
}

/// sim 凭据存储（内存档）：sim 是显式档不是持久化承载——进程内存态即"在册"，
/// 进程重启即失（sim 档无"重启仍在"语义，如实，不装）。
#[derive(Default)]
pub struct Rr3SimWifiStore {
    ssid: Mutex<Option<String>>,
}

impl Rr3WifiStore for Rr3SimWifiStore {
    fn rr3_wifi_store_connect(&self, ssid: &str, _pass: &str) -> Result<(), Rr3HalError> {
        *self
            .ssid
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(ssid.to_string());
        Ok(())
    }

    fn rr3_wifi_store_forget(&self) -> Result<(), Rr3HalError> {
        *self
            .ssid
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }

    fn rr3_wifi_store_ssid(&self) -> Result<Option<Rr3SsidText>, Rr3HalError> {
        Ok(self
            .ssid
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .map(|s| Rr3SsidText::rr3_ssid_text_new(&s)))
    }
}

/// 占位凭据存储：能力缺失显式回错（同 [`Rr3UnavailableSource`](rr-hal-api) 口径
/// ——不静默换后端，选择面落 placeholder 时使用处 Unsupported 点名）。
pub struct Rr3WifiStoreUnavailable;

impl Rr3WifiStore for Rr3WifiStoreUnavailable {
    fn rr3_wifi_store_connect(&self, _ssid: &str, _pass: &str) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/wifi-store"))
    }
    fn rr3_wifi_store_forget(&self) -> Result<(), Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/wifi-store"))
    }
    fn rr3_wifi_store_ssid(&self) -> Result<Option<Rr3SsidText>, Rr3HalError> {
        Err(Rr3HalError::Unsupported("placeholder/wifi-store"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 非计时用例的夹具超时档（生产 config 同档），见 nmcli_support 常量 doc——
    // 逻辑用例不因 spawn 负载抖动而假红；100 ms 档由超时守护用例自带
    use crate::nmcli_support::RR3_TEST_NM_TIMEOUT;

    /// 唯一 temp 目录（pid + 纳秒后缀，批H5 教训：只 pid 会撞上一轮遗留目录）
    fn rr3_test_prov_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rr3-hal-prov-{}-{nanos}-{tag}", std::process::id()))
    }

    /// ETXTBSY 瞬态重试（linux_link 同款：fs::write 假脚本后立刻 spawn，
    /// ubuntu runner 偶发 Text file busy）
    fn rr3_test_txtbsy_retry<T>(mut f: impl FnMut() -> Result<T, Rr3HalError>) -> Result<T, Rr3HalError> {
        let mut attempts = 0;
        loop {
            match f() {
                Err(e) if attempts < 2 && e.to_string().contains("Text file busy") => {
                    attempts += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
                other => return other,
            }
        }
    }

    /// 假 nmcli：自 `export NMCONF_HOME=<conf>`（测试进程零 set_var 竞态——
    /// 环境只进子进程），以 $NMCONF_HOME/rr3-provision 为连接配置文件模拟
    /// NM 的持久化语义：add/modify 写文件、delete 删文件、在册查询列文件。
    fn rr3_test_nm_script(tag: &str, extra: &str) -> (PathBuf, PathBuf) {
        let dir = rr3_test_prov_dir(tag);
        let conf = dir.join("nmconf");
        std::fs::create_dir_all(&conf).unwrap();
        let script = dir.join("fake-nmcli");
        let logic = r#"F="$NMCONF_HOME/rr3-provision"
if [ "$1" = "-t" ]; then
  if [ "$3" = "NAME" ]; then
    [ -f "$F" ] && printf 'rr3-provision\n'
    exit 0
  fi
  if [ "$3" = "802-11-wireless.ssid" ]; then
    if [ -f "$F" ]; then printf '802-11-wireless.ssid:%s\n' "$(cut -d'|' -f1 "$F")"; else printf '802-11-wireless.ssid:\n'; fi
    exit 0
  fi
  echo "bad terse args: $3" >&2
  exit 9
fi
if [ "$1" = "con" ]; then
  if [ "$2" = "delete" ]; then
    rm -f "$F"
    exit 0
  fi
  ssid=""; psk=""; prev=""
  for a in "$@"; do
    case "$prev" in
      802-11-wireless.ssid) ssid="$a" ;;
      802-11-wireless-security.psk) psk="$a" ;;
    esac
    prev="$a"
  done
  printf '%s|%s\n' "$ssid" "$psk" > "$F"
  exit 0
fi
echo "bad subcommand: $1" >&2
exit 9
"#;
        std::fs::write(&script, format!("#!/bin/sh\nexport NMCONF_HOME='{}'\n{extra}{logic}", conf.display()))
            .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (script, conf.join(RR3_NM_PROVISION_CON))
    }

    /// MC-6 主判据（工单原文）："connect → 重启 → 仍在册" + "forget 后连接
    /// 配置真消失"——同一 NMCONF_HOME 夹具下重新构造后端 = 重启后查在册。
    #[test]
    fn nm_store_persists_across_reconstruct() {
        let (script, conf_file) = rr3_test_nm_script("persist", "");
        let store = Rr3NmWifiStore::rr3_nm_wifi_store_new(&script, RR3_TEST_NM_TIMEOUT);

        rr3_test_txtbsy_retry(|| store.rr3_wifi_store_connect("home", "password8")).unwrap();
        let on_disk = std::fs::read_to_string(&conf_file).unwrap();
        assert_eq!(
            on_disk.trim(),
            "home|password8",
            "connect 必须把凭据写进 NM 承载的配置文件（后端承载持久化）"
        );

        // "重启"：丢弃旧句柄，凭同一夹具重新构造后端——凭据仍在册
        let store2 = Rr3NmWifiStore::rr3_nm_wifi_store_new(&script, RR3_TEST_NM_TIMEOUT);
        let ssid =
            rr3_test_txtbsy_retry(|| store2.rr3_wifi_store_ssid()).unwrap();
        assert_eq!(
            ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
            Some("home"),
            "重启后仍在册（MC-6 主判据）"
        );

        // forget 真删：配置文件消失，新构造的"重启"视角查不在册
        rr3_test_txtbsy_retry(|| store2.rr3_wifi_store_forget()).unwrap();
        assert!(!conf_file.exists(), "forget 后连接配置真消失");
        let store3 = Rr3NmWifiStore::rr3_nm_wifi_store_new(&script, RR3_TEST_NM_TIMEOUT);
        let ssid = rr3_test_txtbsy_retry(|| store3.rr3_wifi_store_ssid()).unwrap();
        assert!(ssid.is_none(), "forget 后重启查不在册");

        let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
    }

    /// 换网 = 同名 profile 更新（add 后再 connect 走 modify 路径，锚名不变）
    #[test]
    fn nm_store_second_connect_updates_same_profile() {
        let (script, conf_file) = rr3_test_nm_script("update", "");
        let store = Rr3NmWifiStore::rr3_nm_wifi_store_new(&script, RR3_TEST_NM_TIMEOUT);
        rr3_test_txtbsy_retry(|| store.rr3_wifi_store_connect("first", "password1")).unwrap();
        rr3_test_txtbsy_retry(|| store.rr3_wifi_store_connect("second", "password2")).unwrap();
        let on_disk = std::fs::read_to_string(&conf_file).unwrap();
        assert_eq!(on_disk.trim(), "second|password2", "换网更新同一 profile");
        let ssid = rr3_test_txtbsy_retry(|| store.rr3_wifi_store_ssid()).unwrap();
        assert_eq!(ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()), Some("second"));
        let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
    }

    /// 未连先忘幂等：不在册时 forget 直接 Ok（连 delete 都不发）
    #[test]
    fn nm_store_forget_is_idempotent_when_absent() {
        let (script, conf_file) = rr3_test_nm_script("forget-empty", "");
        let store = Rr3NmWifiStore::rr3_nm_wifi_store_new(&script, RR3_TEST_NM_TIMEOUT);
        rr3_test_txtbsy_retry(|| store.rr3_wifi_store_forget()).unwrap();
        assert!(!conf_file.exists());
        let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
    }

    /// NM 空值语义：在册但 SSID 域为 `--`/空 → None，不编造空 SSID
    #[test]
    fn nm_store_ssid_rejects_nm_empty_marker() {
        let (script, conf_file) = rr3_test_nm_script("empty-ssid", "");
        // 手工把 profile 写成空 SSID（模拟 open 网络 profile 无 ssid 域值）
        std::fs::write(&conf_file, "|x\n").unwrap();
        let store = Rr3NmWifiStore::rr3_nm_wifi_store_new(&script, RR3_TEST_NM_TIMEOUT);
        let ssid = rr3_test_txtbsy_retry(|| store.rr3_wifi_store_ssid()).unwrap();
        assert!(ssid.is_none(), "空 SSID 域如实 None: {ssid:?}");
        let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
    }

    /// 超时守护：假 nmcli 睡 3 s，超时 100 ms——到点 kill 且显式回错点名超时值
    #[test]
    fn nm_store_timeout_kills_slow_child() {
        let (script, conf_file) =
            rr3_test_nm_script("tmo", "sleep 3\n");
        let store = Rr3NmWifiStore::rr3_nm_wifi_store_new(&script, Duration::from_millis(100));
        let started = std::time::Instant::now();
        let err = rr3_test_txtbsy_retry(|| store.rr3_wifi_store_ssid()).unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "超时必须提前杀掉，不许等满子进程时长"
        );
        assert!(
            err.to_string().contains("超时") && err.to_string().contains("100ms"),
            "错误要点名超时: {err}"
        );
        let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
    }

    /// 退出非零：显式回错带 stderr 原文，不静默装成功
    #[test]
    fn nm_store_nonzero_exit_fails_loudly() {
        let (script, conf_file) =
            rr3_test_nm_script("fail", "if [ \"$2\" = \"delete\" ]; then echo nm no such profile >&2; exit 4; fi\n");
        let store = Rr3NmWifiStore::rr3_nm_wifi_store_new(&script, RR3_TEST_NM_TIMEOUT);
        rr3_test_txtbsy_retry(|| store.rr3_wifi_store_connect("home", "password8")).unwrap();
        let err = rr3_test_txtbsy_retry(|| store.rr3_wifi_store_forget()).unwrap_err();
        assert!(
            err.to_string().contains("退出非零") && err.to_string().contains("no such profile"),
            "错误要点名退出非零与 stderr: {err}"
        );
        let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
    }

    /// sim 内存档：connect → 在册、forget → 不在册（进程内存态即"在册"，
    /// sim 无"重启仍在"语义——那是持久化承载的事）
    #[test]
    fn sim_store_roundtrips_in_process() {
        let store = Rr3SimWifiStore::default();
        assert!(store.rr3_wifi_store_ssid().unwrap().is_none(), "出厂不在册");
        store.rr3_wifi_store_connect("home", "password8").unwrap();
        assert_eq!(
            store.rr3_wifi_store_ssid().unwrap().map(|s| s.rr3_ssid_text_as_str().to_string()),
            Some("home".to_string())
        );
        store.rr3_wifi_store_forget().unwrap();
        assert!(store.rr3_wifi_store_ssid().unwrap().is_none());
    }

    /// 占位存储：三动作全 Unsupported 点名，不静默装成功
    #[test]
    fn unavailable_store_errors_loudly() {
        let store = Rr3WifiStoreUnavailable;
        assert!(store.rr3_wifi_store_connect("h", "password8").is_err());
        assert!(store.rr3_wifi_store_forget().is_err());
        assert!(store.rr3_wifi_store_ssid().is_err());
    }
}
