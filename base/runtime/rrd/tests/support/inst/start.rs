//! 监听面起法族：从 `rr3_start` 到 `rr3_start_profile_env` 的九个入口。
//!
//! 全部走 `rr3_spawn_inner`（唯一 spawn 实现），差异只在参数组合：叠加层有无、
//! tick 大小、`--manifest` 直驱 / `--profile` 正门、环境变量注入。装配缺格跳过
//! 夹具由 `rr3_listen_profile` 统一铺，返回的夹具根由调用方认领进 `fixture_root`
//! （Drop 清整树）。

use super::*;

impl RrdTest {
    /// 起 rrd：套接字用 pid+tag 唯一名（Global Constraints 8），退避基数压到 20ms
    /// （Task 11 要跑完 1..60 档），注册前读超时 300ms（本任务测试 8 用它）。
    pub fn rr3_start(tag: &str) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        // 就绪判据用日志而非 connect：connect 探测会留下一条"首帧读取失败"噪声日志
        RrdTest::rr3_spawn_with_overlay(
            &socket,
            None,
            &socket,
            Some(Path::new(RR3_OVERLAY_PATH)),
            RR3_QUIET_WATCHDOG_TICK_MS,
        )
    }

    /// **不带** `--topics-overlay` 的起法：在册的只有 `contracts/topics.toml` 那 21 条生产主题。
    /// 专治"叠加层被当成生产放行口"（K4）：夹具命名空间 `rr3-sima/*` 在这里根本不存在，
    /// 而生产命名空间的 owner 又不是夹具模块 → 两种拒法都必须在**生产口径**下成立。
    /// 套接字名与 `rr3_start` 同一套 pid+tag 唯一名规则（Global Constraints 8）。
    /// 基-23 起与 `rr3_start` 共用同一份缺格跳过装配单（装载面正交于叠加层面）。
    pub fn rr3_start_no_overlay(tag: &str) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        RrdTest::rr3_spawn_with_overlay(&socket, None, &socket, None, RR3_QUIET_WATCHDOG_TICK_MS)
    }

    /// `rr3_start_no_overlay` 的带环境变量版（0.0.4 批B）：计数持久化用例经它给
    /// rrd 进程注入 `RR3_DATA_DIR`。不 `std::env::set_var`（与 `rr3_start_profile_env`
    /// 同一纪律：测试进程的 env 是共享的，改自己会串别的用例）。
    pub fn rr3_start_env(tag: &str, envs: &[(&str, &str)]) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        // 基-23：与 rr3_spawn_with_overlay 同一条缺格跳过装配门口径（它不走 overlay 形参、
        // 直接铺 rr3_spawn_inner，所以单独补装配面——两个入口都不许裸起）。
        let (profile, fixture_root) = rr3_listen_profile(&socket);
        let mut me = RrdTest::rr3_spawn_inner(
            &socket,
            None,
            &socket,
            None,
            RR3_QUIET_WATCHDOG_TICK_MS,
            &[],
            None,
            Some(&profile),
            envs,
        );
        me.fixture_root = Some(fixture_root);
        me
    }

    /// **显式 `--manifest` 调试后门**的起法（基-23）：manifest 指向 pid 唯一的**不存在
    /// 路径**——`rr3_boot_job_parse` 打一行"拒绝启动：manifest … 读取失败"后返回
    /// None（E-3：boot 期配置错误不吃预算，读不动 = None 跳过，非 fatal），boot 队列为空，
    /// rrd 照常监听。专供"必须走后门路径"的用例（e2e_ctl 的 ctl.wiring unavailable 断言：
    /// 后门无装配快照，如实回 available:false）——监听面主口径（rr3_start 一族）走的是
    /// 缺格跳过装配单正门，两形状的可观察面同为零 spawn。
    pub fn rr3_start_manifest_stub(tag: &str) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        let stub = std::env::temp_dir().join(format!(
            "rr3-no-such-manifest-{}-{tag}.toml",
            std::process::id()
        ));
        RrdTest::rr3_spawn_inner(
            &socket,
            None,
            &socket,
            None,
            RR3_QUIET_WATCHDOG_TICK_MS,
            &[stub.as_path()],
            None,
            None,
            &[],
        )
    }

    /// watchdog 线级用例的唯一入口。**没有默认值**是刻意的：到期判死必须是用例自己声明的
    /// 条件，不能是"忘了传参数"传出来的。
    pub fn rr3_start_with_tick(tag: &str, tick_ms: u64) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        RrdTest::rr3_spawn_with_overlay(
            &socket,
            None,
            &socket,
            Some(Path::new(RR3_OVERLAY_PATH)),
            tick_ms,
        )
    }

    /// 带 boot spawn 的起法：`manifests` 原样进 `--manifest`（顺序即 CLI 序，spawn 序由
    /// start_priority 重排），`boot_role` 进 `--boot-role`。**tick 必传**（与
    /// `rr3_start_with_tick` 同一理由：到期判死是用例自己声明的条件）。
    ///
    /// **但这一处的 tick 不只是"判死节拍"**：Task 11b 的 E-5 让 supervisor 的**扫尸节拍**
    /// 复用同一个旋钮（`recv_timeout(sweep)`），所以拿 `RR3_QUIET_WATCHDOG_TICK_MS` 起 boot
    /// 用例会让收尸一小时不发生 —— boot 用例**必须**传小 tick（`E2E_TICK_MS`）。
    /// 这条耦合是指据不是坑：它钉住了"全 daemon 只有一条执行线程 + 一个节拍旋钮"。
    pub fn rr3_start_boot(
        tag: &str,
        manifests: &[&Path],
        boot_role: &str,
        tick_ms: u64,
    ) -> RrdTest {
        RrdTest::rr3_start_boot_env(tag, manifests, boot_role, tick_ms, &[])
    }

    /// `rr3_start_boot` 的带环境变量版（0.0.6 批F2）：`envs` 原样注入 rrd 进程环境，
    /// 经 spawn 继承传给 `--manifest` 直驱 boot 的**真模块**——判向 e2e 用它给
    /// rr3-sense 注入 `RR3_HAL_SIM_DOA_DEG`（与 `rr3_start_profile_env` 同一条纪律）。
    pub fn rr3_start_boot_env(
        tag: &str,
        manifests: &[&Path],
        boot_role: &str,
        tick_ms: u64,
        envs: &[(&str, &str)],
    ) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        RrdTest::rr3_spawn_inner(
            &socket,
            None,
            &socket,
            Some(Path::new(RR3_OVERLAY_PATH)),
            tick_ms,
            manifests,
            Some(boot_role),
            None,
            envs,
        )
    }

    /// 装配单正门的起法（0.0.2 D2/T3）：`profile` 原样进 `--profile`。**tick 必传**
    /// （与 `rr3_start_boot` 同一理由：supervisor 的扫尸节拍复用同一旋钮，boot 用例必须小 tick）。
    pub fn rr3_start_profile(tag: &str, profile: &Path, tick_ms: u64) -> RrdTest {
        RrdTest::rr3_start_profile_env(tag, profile, tick_ms, &[])
    }

    /// `rr3_start_profile` 的带环境变量版（0.0.2 T4b）：`envs` 原样注入 rrd 进程环境，
    /// 并经 `Command::spawn` 的继承语义传给 rrd spawn 的**真模块**——sim 后端设备参数
    /// （如 `RR3_HAL_SIM_BATTERY_PERCENT`）的唯一注入通道。e2e 不用 `std::env::set_var`
    ///（unsafe 且与并行测试竞态，rr-hal backend.rs 同一条禁令），一律走这里显式传。
    pub fn rr3_start_profile_env(
        tag: &str,
        profile: &Path,
        tick_ms: u64,
        envs: &[(&str, &str)],
    ) -> RrdTest {
        let socket = PathBuf::from(format!("/tmp/rr3-m2-{}-{tag}.sock", std::process::id()));
        RrdTest::rr3_spawn_inner(
            &socket,
            None,
            &socket,
            None,
            tick_ms,
            &[],
            None,
            Some(profile),
            envs,
        )
    }
}
