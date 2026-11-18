//! provision_store 单测（文件头：本文件只放测试，生产面在 `provision_store.rs`）。
//!
//! 拆分动因：`provision_store.rs` 499 行、内联测试自 237 行起占近半，先按仓内同形搬出。

use super::*;
// 非计时用例的夹具超时档（生产 config 同档），见 nmcli_support 常量 doc——
// 逻辑用例不因 spawn 负载抖动而假红；100 ms 档由超时守护用例自带
use crate::nmcli_support::RR3_TEST_NM_TIMEOUT;

/// 凭据形制校验：合法边界通过，超限/前导 '-'/控制字符/空值逐条拒。
#[test]
fn wifi_cred_check_rejects_unsafe_shapes() {
    assert!(rr3_wifi_cred_check("Home-5G", "password8").is_ok());
    assert!(rr3_wifi_cred_check(&"s".repeat(32), &"p".repeat(63)).is_ok());
    assert!(rr3_wifi_cred_check("", "password8").is_err(), "空 SSID");
    assert!(
        rr3_wifi_cred_check(&"s".repeat(33), "password8").is_err(),
        "SSID 超限"
    );
    assert!(rr3_wifi_cred_check("Home", "").is_err(), "空密码");
    assert!(
        rr3_wifi_cred_check("Home", &"p".repeat(64)).is_err(),
        "密码超限"
    );
    assert!(
        rr3_wifi_cred_check("-t", "password8").is_err(),
        "SSID 前导 '-'"
    );
    assert!(
        rr3_wifi_cred_check("Home", "--help").is_err(),
        "密码前导 '-'"
    );
    assert!(
        rr3_wifi_cred_check("Ho\u{1}me", "password8").is_err(),
        "控制字符"
    );
}

/// 唯一 temp 目录（pid + 纳秒后缀，批H5 教训：只 pid 会撞上一轮遗留目录）
#[cfg(unix)]
fn rr3_test_prov_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("rr3-hal-prov-{}-{nanos}-{tag}", std::process::id()))
}

/// ETXTBSY 瞬态重试（linux_link 同款：fs::write 假脚本后立刻 spawn，
/// ubuntu runner 偶发 Text file busy）
#[cfg(unix)]
fn rr3_test_txtbsy_retry<T>(
    mut f: impl FnMut() -> Result<T, Rr3HalError>,
) -> Result<T, Rr3HalError> {
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
///
/// 门控 `unix`：夹具是**可执行 sh 脚本**（靠权限位 0o755 被 spawn），
/// 非 Unix 无执行面——Windows 原生测试编译面由 2026-11-16 平台腿暴露，
/// 该能力判据整体按 unix 门控（不是打补丁式跳测）。
#[cfg(unix)]
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
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nexport NMCONF_HOME='{}'\n{extra}{logic}",
            conf.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (script, conf.join(RR3_NM_PROVISION_CON))
}

/// MC-6 主判据（工单原文）："connect → 重启 → 仍在册" + "forget 后连接
/// 配置真消失"——同一 NMCONF_HOME 夹具下重新构造后端 = 重启后查在册。
#[cfg(unix)]
#[test]
fn nm_store_persists_across_reconstruct() {
    let (script, conf_file) = rr3_test_nm_script("persist", "");
    let store = Rr3NmWifiStore::rr3_nm_wifi_new(&script, RR3_TEST_NM_TIMEOUT);

    rr3_test_txtbsy_retry(|| store.rr3_wifi_connect("home", "password8")).unwrap();
    let on_disk = std::fs::read_to_string(&conf_file).unwrap();
    assert_eq!(
        on_disk.trim(),
        "home|password8",
        "connect 必须把凭据写进 NM 承载的配置文件（后端承载持久化）"
    );

    // "重启"：丢弃旧句柄，凭同一夹具重新构造后端——凭据仍在册
    let store2 = Rr3NmWifiStore::rr3_nm_wifi_new(&script, RR3_TEST_NM_TIMEOUT);
    let ssid = rr3_test_txtbsy_retry(|| store2.rr3_wifi_ssid()).unwrap();
    assert_eq!(
        ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
        Some("home"),
        "重启后仍在册（MC-6 主判据）"
    );

    // forget 真删：配置文件消失，新构造的"重启"视角查不在册
    rr3_test_txtbsy_retry(|| store2.rr3_wifi_forget()).unwrap();
    assert!(!conf_file.exists(), "forget 后连接配置真消失");
    let store3 = Rr3NmWifiStore::rr3_nm_wifi_new(&script, RR3_TEST_NM_TIMEOUT);
    let ssid = rr3_test_txtbsy_retry(|| store3.rr3_wifi_ssid()).unwrap();
    assert!(ssid.is_none(), "forget 后重启查不在册");

    let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
}

/// 换网 = 同名 profile 更新（add 后再 connect 走 modify 路径，锚名不变）
#[cfg(unix)]
#[test]
fn nm_store_second_connect_updates_same_profile() {
    let (script, conf_file) = rr3_test_nm_script("update", "");
    let store = Rr3NmWifiStore::rr3_nm_wifi_new(&script, RR3_TEST_NM_TIMEOUT);
    rr3_test_txtbsy_retry(|| store.rr3_wifi_connect("first", "password1")).unwrap();
    rr3_test_txtbsy_retry(|| store.rr3_wifi_connect("second", "password2")).unwrap();
    let on_disk = std::fs::read_to_string(&conf_file).unwrap();
    assert_eq!(on_disk.trim(), "second|password2", "换网更新同一 profile");
    let ssid = rr3_test_txtbsy_retry(|| store.rr3_wifi_ssid()).unwrap();
    assert_eq!(
        ssid.as_ref().map(|s| s.rr3_ssid_text_as_str()),
        Some("second")
    );
    let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
}

/// 未连先忘幂等：不在册时 forget 直接 Ok（连 delete 都不发）
#[cfg(unix)]
#[test]
fn nm_store_forget_is_idempotent_when_absent() {
    let (script, conf_file) = rr3_test_nm_script("forget-empty", "");
    let store = Rr3NmWifiStore::rr3_nm_wifi_new(&script, RR3_TEST_NM_TIMEOUT);
    rr3_test_txtbsy_retry(|| store.rr3_wifi_forget()).unwrap();
    assert!(!conf_file.exists());
    let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
}

/// NM 空值语义：在册但 SSID 域为 `--`/空 → None，不编造空 SSID
#[cfg(unix)]
#[test]
fn nm_store_ssid_rejects_nm_empty_marker() {
    let (script, conf_file) = rr3_test_nm_script("empty-ssid", "");
    // 手工把 profile 写成空 SSID（模拟 open 网络 profile 无 ssid 域值）
    std::fs::write(&conf_file, "|x\n").unwrap();
    let store = Rr3NmWifiStore::rr3_nm_wifi_new(&script, RR3_TEST_NM_TIMEOUT);
    let ssid = rr3_test_txtbsy_retry(|| store.rr3_wifi_ssid()).unwrap();
    assert!(ssid.is_none(), "空 SSID 域如实 None: {ssid:?}");
    let _ = std::fs::remove_dir_all(conf_file.parent().unwrap().parent().unwrap());
}

/// 超时守护：假 nmcli 睡 3 s，超时 100 ms——到点 kill 且显式回错点名超时值
#[cfg(unix)]
#[test]
fn nm_store_timeout_kills_slow_child() {
    let (script, conf_file) = rr3_test_nm_script("tmo", "sleep 3\n");
    let store = Rr3NmWifiStore::rr3_nm_wifi_new(&script, Duration::from_millis(100));
    let started = std::time::Instant::now();
    let err = rr3_test_txtbsy_retry(|| store.rr3_wifi_ssid()).unwrap_err();
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
#[cfg(unix)]
#[test]
fn nm_store_nonzero_exit_fails_loudly() {
    let (script, conf_file) = rr3_test_nm_script(
        "fail",
        "if [ \"$2\" = \"delete\" ]; then echo nm no such profile >&2; exit 4; fi\n",
    );
    let store = Rr3NmWifiStore::rr3_nm_wifi_new(&script, RR3_TEST_NM_TIMEOUT);
    rr3_test_txtbsy_retry(|| store.rr3_wifi_connect("home", "password8")).unwrap();
    let err = rr3_test_txtbsy_retry(|| store.rr3_wifi_forget()).unwrap_err();
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
    assert!(store.rr3_wifi_ssid().unwrap().is_none(), "出厂不在册");
    store.rr3_wifi_connect("home", "password8").unwrap();
    assert_eq!(
        store
            .rr3_wifi_ssid()
            .unwrap()
            .map(|s| s.rr3_ssid_text_as_str().to_string()),
        Some("home".to_string())
    );
    store.rr3_wifi_forget().unwrap();
    assert!(store.rr3_wifi_ssid().unwrap().is_none());
}

/// 占位存储：三动作全 Unsupported 点名，不静默装成功
#[test]
fn unavailable_store_errors_loudly() {
    let store = Rr3WifiStoreUnavailable;
    assert!(store.rr3_wifi_connect("h", "password8").is_err());
    assert!(store.rr3_wifi_forget().is_err());
    assert!(store.rr3_wifi_ssid().is_err());
}
