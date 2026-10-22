use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use esp_idf_hal::uart::UartDriver;
use esp_idf_sys::{esp_random, esp_timer_get_time};
use rr_ipc::{Rr3BoardBindings, Rr3ByteStream};

use crate::board::{wander_step, Axis};
use crate::link::{reply, UartIo};
use crate::power::RailTier;

/// esp_timer 毫秒（单调时基，微动作时钟源；u64 毫秒 58 万年不回绕）。
pub(crate) fn now_ms() -> u64 {
    (unsafe { esp_timer_get_time() }).unsigned_abs() / 1000
}

/// 硬件 RNG（WIFI/蓝牙未启也能用——ESP32 硬件熵源独立于无线栈）。
pub(crate) fn esp_random_u32() -> u32 {
    unsafe { esp_random() }
}

// ---------------------------------------------------------------------------
// 板侧绑定单点（rr3.set_bindings 注入态，批N4 件七(c) / 批N9 件五）：宿主经桥上
// method 下发的引脚真值（esp32-mini [hardware] 十键 = 批N4 三键自愈/观测面 +
// 批N9 件五七键主驱动对账面 → rrd RR3_HAL_BINDINGS → supervisor env → 桥），落
// 此进程级单例。引脚号是芯片级物理事实，且 I2C 自愈的裸寄存器路径在深层自由函数
// （[`crate::oled`]，够不到 `Bus`）——故不随 `PeriphState` 走，单点存取。
// **消费面 fail-closed：未注入（None）即拒不动作**，观测面在调用方
// （boot 解卡点名 / 运行面 `selfheal_skipped` 计数 / 注入到达时装配对账
// `binding_mismatch` 计数，见 [`crate::board::rr3_bindings_reconcile`]）。
// ---------------------------------------------------------------------------

/// 板侧绑定单点（`None` = 未注入）。毒锁不失效（同 [`lock_bus`] 纪律）。
static BOARD_BINDINGS: Mutex<Option<Rr3BoardBindings>> = Mutex::new(None);

/// 覆盖写板侧绑定单点（set_bindings 分发面用；全量覆盖语义，与宿主
/// `rr3_push_bindings` 每次连接重推全量一致）。
pub(crate) fn rr3_board_bindings_store(b: Rr3BoardBindings) {
    *BOARD_BINDINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(b);
}

/// 读 I2C 引脚对 `(sda, scl)`（自愈路径消费点用；缺任一键或未注入回 None）。
pub(crate) fn rr3_board_bindings_i2c_pair() -> Option<(u8, u8)> {
    BOARD_BINDINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(Rr3BoardBindings::rr3_board_bindings_i2c)
}

/// 控制面/运行面共享状态（[`Bus`] 内，`Arc<Mutex<..>>` 护）：行面/帧面的文本与
/// 角度记账 + 自主行为（表情/扫摆）时钟。**不含任何硬件句柄**（故自动 Send）——
/// 控制面改策略字段，运行面读之执行 + 回写自主推进的角度/拍号。帧面 expression
/// 应答的 previous 字段不在本结构（命令由控制面串行化，记账在 [`CtlState`]）。
pub(crate) struct PeriphState {
    /// 最近一次 OLED 文本（行面 OLED 命令 / 帧面 expression 出字）
    pub(crate) last_text: String,
    pub(crate) pan_deg: u32,
    pub(crate) tilt_deg: u32,
    /// 显示模式：true=帧面 64x64 点阵位图占全屏（舵机角度行不刷，不凿位图）；
    /// 行面 OLED 命令 / 帧面 expression 复位 false。
    pub(crate) frame_mode: bool,
    /// 表情模式：true=胶囊眼 + 生命感微动作（帧面 expression 专有）；位图占屏
    /// 与行面文本模式复位 false（动画只在此模式跑）。
    pub(crate) expr_mode: bool,
    /// 表情基态（expr_eye_base 预设）：眼开合% + 水平/垂直扫视偏移
    pub(crate) base_open: u32,
    pub(crate) base_dx: i16,
    pub(crate) base_dy: i16,
    /// 微动作时钟（esp_timer ms）：眨眼/扫视的下一触发点与进行中截止点 +
    /// 上次渲染合成参数（变化才重绘，省 I2C）。
    pub(crate) next_blink_ms: u64,
    pub(crate) blink_until_ms: u64,
    pub(crate) next_glance_ms: u64,
    pub(crate) glance_until_ms: u64,
    pub(crate) glance_dx: i16,
    pub(crate) glance_dy: i16,
    pub(crate) drawn: (u32, i16, i16),
    /// 云台自主扫摆（WANDER 行面开关）：pan/tilt 缓速连续摆动不停顿。外部
    /// PAN/TILT/SERVO/SCAN 或帧面 servo 写到达即关（外部点名优先）。
    pub(crate) wander_on: bool,
    pub(crate) wander_pan_dir: i8,
    pub(crate) wander_tilt_dir: i8,
    /// 扫摆节拍：60ms 一拍的拍号（u8 回绕无妨，只用模数）+ 下拍时间。
    pub(crate) wander_phase: u8,
    pub(crate) next_wander_ms: u64,
    /// 5V 轨最近一次采样（方案A Q1 观测面，由**运行面**写入）：轨压 mV + 引脚 raw。
    /// `0` = 尚未采样（boot 初值/未装配）。
    pub(crate) rail_mv: u16,
    pub(crate) rail_raw: u16,
    /// 5V 轨最近档位（方案A Q2，由**运行面**采样算出；`INFO` 观测 + 驱动面拒动判据）。
    /// boot 初值 `Ok`（未采样前不误判拒动）。
    pub(crate) rail_tier: RailTier,
    /// 5V 轨越阈事件计数（低电段首发次数，只增不减；`INFO` 观测）。
    pub(crate) rail_low_events: u32,
    /// 音频上行开关（`AUDIO ON|OFF` 行命令置位；mic 线每 read 批读——false 时
    /// 只出统计冒烟面不攒分片）。
    pub(crate) audio_on: bool,
    /// 音频分片序号（wrapping u32，随分片自增；recorder 按 seq 连续性计丢片，
    /// 与 `audio_dropped` 互为对账）。
    pub(crate) audio_seq: u32,
    /// 音频已上行分片数（audio_q 出队整帧写全即计；`INFO` 观测）。
    pub(crate) audio_sent: u64,
    /// 音频背压丢弃分片数（audio_q 满丢最旧即计；`INFO` 观测）。
    pub(crate) audio_dropped: u64,
    /// I2C 自愈因绑定未注入而跳过的拍数（运行面每拍 force 重同步遇
    /// `rr3_board_bindings_i2c_pair()` 缺席即计，`INFO` 观测；非零即绑定下发链
    /// 未通——fail-closed 的观测面，注入到达即停增、恢复自愈）。
    pub(crate) selfheal_skipped: u32,
    /// 绑定对账不齐键数累计（批N9 件五装配对账锚观测面）：set_bindings 注入到达
    /// 时逐键判等板侧具名装配真值（[`crate::board::rr3_bindings_reconcile`]），
    /// 缺席/失配逐键点名并累计，`INFO` 观测；非零即装配单/下发链漂移（fail-visible，
    /// 不拒收不拒 boot），注入值 = 装配时计数不增。
    pub(crate) binding_mismatch: u32,
    /// 舵机移动失败累计（批N12 件一，敞口 16 第一问缺码半边观测面）：Move 作业
    /// 失败与 WANDER 扫摆驱动失败各计一次（boot 以来只增不减；`INFO` 观测，同
    /// `selfheal_skipped` 静默口径——Move 失败另走 outbox `ERR` 行回宿主，SCAN
    /// 失败已有 outbox 回包不重复计）。
    pub(crate) move_fail_events: u32,
}

impl PeriphState {
    /// 开机初值（中位 90 / 文本 "(boot)" / 无表情无扫摆）。
    pub(crate) fn boot() -> Self {
        Self {
            last_text: String::from("(boot)"),
            // 初值 = 各轴中位锚（生成表 center，批N3 件五：行程真值收敛档案）
            pan_deg: Axis::Pan.center_deg(),
            tilt_deg: Axis::Tilt.center_deg(),
            frame_mode: false,
            expr_mode: false,
            base_open: 100,
            base_dx: 0,
            base_dy: 0,
            next_blink_ms: 0,
            blink_until_ms: 0,
            next_glance_ms: 0,
            glance_until_ms: 0,
            glance_dx: 0,
            glance_dy: 0,
            drawn: (100, 0, 0),
            wander_on: false,
            wander_pan_dir: 0,
            wander_tilt_dir: 0,
            wander_phase: 0,
            next_wander_ms: 0,
            rail_mv: 0,
            rail_raw: 0,
            rail_tier: RailTier::Ok,
            rail_low_events: 0,
            audio_on: false,
            audio_seq: 0,
            audio_sent: 0,
            audio_dropped: 0,
            selfheal_skipped: 0,
            binding_mismatch: 0,
            move_fail_events: 0,
        }
    }

    /// 角度行是否该刷：帧模式/表情模式占满 8 页，刷角度行会凿穿画面（跳过）。
    pub(crate) fn angle_row_visible(&self) -> bool {
        !(self.frame_mode || self.expr_mode)
    }

    /// 表情切换时重置微动作时钟：立即显基态，眨眼/扫视各按随机间隔推迟。
    pub(crate) fn anim_reset(&mut self) {
        let now = now_ms();
        self.blink_until_ms = 0;
        self.glance_until_ms = 0;
        self.next_blink_ms = now + 2800 + (esp_random_u32() % 2400) as u64;
        self.next_glance_ms = now + 4000 + (esp_random_u32() % 4000) as u64;
    }

    /// 表情微动作决策（**纯计算，不碰硬件**——运行面决策段持锁调用）：自动眨眼
    /// （2.8–5.2s 随机间隔，闭 130ms）+ 随机微扫视（4–8s 间隔，水平 ±6px、垂直
    /// ±2 行，500–900ms 回位）。返回需要重绘的合成参数（None = 与上次一致，
    /// 省 I2C 不重绘）；非表情模式恒 None。渲染由运行面脱锁执行。
    pub(crate) fn anim_decide(&mut self) -> Option<(u32, i16, i16)> {
        if !self.expr_mode {
            return None;
        }
        let now = now_ms();
        if self.blink_until_ms == 0 {
            if now >= self.next_blink_ms {
                self.blink_until_ms = now + 130;
                self.next_blink_ms = now + 2800 + (esp_random_u32() % 2400) as u64;
            }
        } else if now >= self.blink_until_ms {
            self.blink_until_ms = 0;
        }
        let open = if self.blink_until_ms != 0 {
            4
        } else {
            self.base_open
        };
        let mut dx = self.base_dx;
        let mut dy = self.base_dy;
        if self.glance_until_ms == 0 {
            if now >= self.next_glance_ms {
                self.glance_dx = (esp_random_u32() % 13) as i16 - 6;
                self.glance_dy = match esp_random_u32() % 3 {
                    0 => -2,
                    1 => 2,
                    _ => 0,
                };
                self.glance_until_ms = now + 500 + (esp_random_u32() % 400) as u64;
                self.next_glance_ms = now + 4000 + (esp_random_u32() % 4000) as u64;
            }
        } else if now >= self.glance_until_ms {
            self.glance_until_ms = 0;
        } else {
            dx += self.glance_dx;
            dy += self.glance_dy;
        }
        let key = (open, dx, dy);
        if key != self.drawn {
            self.drawn = key;
            Some(key)
        } else {
            None
        }
    }

    /// 云台自主扫摆决策（**纯计算**，60ms 一拍）：pan 每 2 拍走 2°（≈16.7°/s，
    /// 行程 40° 单程约 2.4s）、tilt 每 4 拍走 3°（≈12.5°/s，行程 10° 单程约 1s），
    /// 端点折返持续不停。**端点 = 本轴行程边界**（[`Axis::range`]，方案 C 分轴限幅：
    /// pan 中位±30° / tilt 中位±10°）——旧版折返点在 0/180，会把舵机推过机械死点
    /// 并持续施力（真板表现为卡死 + 发烫）。步幅下限实测定标：tilt 托整个云台上部
    /// （重载），2°/300ms 部分落死区里视觉近乎不动——tilt 用 3°（≈27 ticks）可见。
    /// **串行降级：同一拍只驱动一轴**（tilt 拍占位时该拍跳过 pan）——两轴错拍，
    /// 削并行峰值电流（适配板载 5V 直供）。返回本拍要点名的单轴动作（None=不动）
    /// + 是否刷角度行（每 20 拍 = 1.2s 一次，省 I2C；模式守卫在刷新处再判）。
    pub(crate) fn wander_decide(&mut self) -> (Option<(Axis, u32)>, bool) {
        if !self.wander_on || now_ms() < self.next_wander_ms {
            return (None, false);
        }
        self.next_wander_ms = now_ms() + 60;
        self.wander_phase = self.wander_phase.wrapping_add(1);
        let mv = if self.wander_phase % 4 == 0 {
            let next = wander_step(
                self.tilt_deg,
                &mut self.wander_tilt_dir,
                3,
                Axis::Tilt.range(),
            );
            self.tilt_deg = next;
            Some((Axis::Tilt, next))
        } else if self.wander_phase % 2 == 0 {
            let next = wander_step(self.pan_deg, &mut self.wander_pan_dir, 2, Axis::Pan.range());
            self.pan_deg = next;
            Some((Axis::Pan, next))
        } else {
            None
        };
        (mv, self.wander_phase % 20 == 0)
    }
}

// ---------------------------------------------------------------------------
// 控制面 ↔ 运行面共享总线（Arc<Mutex>；作业/回包队列 + 共享状态）
// ---------------------------------------------------------------------------

/// 控制面 ↔ 运行面共享总线（哨兵 `Arc<Mutex>` 纪律同款）：`jobs` = 控制→运行的
/// 硬件作业队列，`outbox` = 运行→控制的异步回包行队列（SCAN 逐点），`st` = 双面
/// 共享状态。**锁只护读写，绝不跨 I2C 持有**（运行面"决策段持锁纯算，渲染段脱锁"）。
pub(crate) struct Bus {
    pub(crate) jobs: VecDeque<Job>,
    pub(crate) outbox: VecDeque<String>,
    /// 音频上行队列（mic 线产 → 控制面 drain；元素 = 整帧字节，出队即写 UART2。
    /// 有界 [`crate::board::AUDIO_Q_CAP`]，满丢最旧 + 计 `audio_dropped`）。
    pub(crate) audio_q: VecDeque<Vec<u8>>,
    pub(crate) st: PeriphState,
}

/// 控制面 → 运行面的硬件作业（**控制面只推作业，不碰硬件**；运行面取之执行）。
pub(crate) enum Job {
    /// 依 `st` 重绘文本 3 行 + 角度行（OLED 命令 / 舵机命令后角度行随动；模式守卫）。
    RedrawOled,
    /// 舵机移动（**串行降级**：双轴 = 先 pan 到位 → 整定 → 再 tilt；单轴直动）。
    Move { pan: Option<u32>, tilt: Option<u32> },
    /// 五点串行扫描（判据②；逐点异步回包走 outbox）。
    Scan,
    /// 依 `st` 基态渲染胶囊眼（表情切换/初态；后续微动作走自主节拍）。
    RenderCapsules,
    /// 渲染 8x8 点阵全屏放大。
    RenderFrame([u8; 8]),
    /// 采样一次 5V 轨（RAIL 行命令点名；跑在运行面，结果走 outbox 异步回包）。
    SampleRail,
}

/// 控制面私有状态（不跨线程）：帧面 expression 应答的 previous 字段——命令由控制面
/// 串行化，故 previous 记账在控制面即真值（无需随 `st` 跨线程）。
pub(crate) struct CtlState {
    pub(crate) prev_expr: String,
}

/// 取总线锁（中毒不失效：std 惰性 pthread 面，哨兵同款 `into_inner` 纪律）。
pub(crate) fn lock_bus(bus: &Arc<Mutex<Bus>>) -> std::sync::MutexGuard<'_, Bus> {
    bus.lock().unwrap_or_else(|p| p.into_inner())
}

/// 控制面推一件作业（唤醒运行面；运行面每轮取队列头执行）。
pub(crate) fn push_job(bus: &Arc<Mutex<Bus>>, job: Job) {
    lock_bus(bus).jobs.push_back(job);
}

/// 运行面 → 控制面的异步回包行（SCAN 逐点等；控制面每轮取空）。
pub(crate) fn outbox_push(bus: &Arc<Mutex<Bus>>, line: String) {
    lock_bus(bus).outbox.push_back(line);
}

/// 取空 outbox 并回写 UART2（控制面每轮首步；异步回包不阻塞命令吞吐）。
pub(crate) fn drain_outbox(uart: &mut UartDriver<'static>, bus: &Arc<Mutex<Bus>>) {
    loop {
        let line = lock_bus(bus).outbox.pop_front();
        match line {
            Some(l) => reply(uart, &l),
            None => break,
        }
    }
}

/// 取空 audio_q 并整帧写 UART2（控制面每轮首步，紧跟 [`drain_outbox`]——同线程
/// 串行写，与行面/应答字节在线上互不交错）。单分片 ~11KB ≈ 120ms@921600（整帧
/// write_all + wait_tx_done，[`UartIo`] 语义），发送期间命令面短延迟是分片粒度
/// 的既定代价（Q1=(a) 带宽账口径）。写不全（IO 失败）不计 `audio_sent`——线上
/// 半帧由帧层拒收自愈，recorder 端 seq 断档对账。
pub(crate) fn drain_audio(uart: &mut UartDriver<'static>, bus: &Arc<Mutex<Bus>>) {
    let mut uio = UartIo { uart, pre: None };
    loop {
        let frame = lock_bus(bus).audio_q.pop_front();
        let Some(f) = frame else { break };
        if uio.rr3_write_all_bytes(&f) == f.len() {
            lock_bus(bus).st.audio_sent += 1;
        }
    }
}
