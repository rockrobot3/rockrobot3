use std::thread;
use std::time::Duration;

use esp_idf_hal::i2c::I2cDriver;

use crate::board::{
    I2C_CHUNK_GAP_MS, I2C_PAGE_ATTEMPTS, I2C_RETRY, I2C_RETRY_DELAY_MS, OLED_ADDR, OLED_COLS,
    OLED_CONTRAST_LEVELS, OLED_ROWS,
};
use crate::face::{eye_page, EyeStyle};
use periph_mech::oled_codec::{
    rr3_frame_pages, rr3_init_commands, rr3_page_cmd, rr3_text_page, RR3_OLED_ZERO_PAGE,
};
use rr_periph_proto::ports::Rr3I2cPort;

use crate::ports_impl::rr3_esp_i2c;
use crate::state::rr3_board_bindings_i2c_pair;

// 字模与编码真值已迁 periph-mech（P0-A A3）：本档只留 I2C 事务/重试/总线自愈。
// 保留 `glyph` 别名供行面 FONT 自检直接取字模（与编码面同一张表，无第二份）。
pub(crate) use periph_mech::oled_codec::rr3_glyph as glyph;


// ---------------------------------------------------------------------------
// SSD1306 原语驱动（命令流 0x00 控制字节 / 数据流 0x40 控制字节；页寻址模式）
// ---------------------------------------------------------------------------

/// 带重试的 I2C 写（**总线自愈 + 瞬态兜底**）：失败先 [`i2c_bus_recover`] 解卡再重发，
/// 至多 [`I2C_RETRY`] 次、每次隔 [`I2C_RETRY_DELAY_MS`]。SSD1306 每笔
/// START-addr-bytes-STOP 独立，重发安全。成功即返回；全败才回 Err（文案带重试次数
/// 与末次错因，便于 monitor 判因）。
fn i2c_write_retry(i2c: &mut impl Rr3I2cPort, addr: u8, bytes: &[u8]) -> Result<(), String> {
    let mut last = String::new();
    for attempt in 0..=I2C_RETRY {
        match i2c.rr3_i2c_write(addr, bytes) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = e;
                if attempt < I2C_RETRY {
                    // 失败即先自愈总线再退避重发（丢拍锁死是本工程主故障模式）
                    i2c_bus_recover(false);
                    thread::sleep(Duration::from_millis(I2C_RETRY_DELAY_MS));
                }
            }
        }
    }
    Err(format!("（重试 {I2C_RETRY} 次后）{last}"))
}

/// I2C 总线自愈（**运行期恢复，与启动期 [`i2c_bus_unstick`] 同机理**）：SSD1306 在
/// 某笔事务中途丢拍（总线噪声/轨压瞬降）时会把自己锁在"等后续时钟"状态——此后主机
/// 每次 START 它一律不应答（真板实录：8 页胶囊眼密集页写中一笔数据段失败后，其后
/// **所有**事务恒 `ESP_FAIL`，重试 6 次 × 5ms 不复、锁死到复位；而短命令笔与 boot
/// 4 页因未撞上丢拍恒过）。恢复动作三步：
/// 1) 把 SDA/SCL 抢回 GPIO（IOMUX 由 I2C 切到 GPIO 功能）；
/// 2) 若 SDA 被从机拉住 → 手动打 9 个 SCL 时钟走完其位计数器，再补 STOP 释放；
/// 3) `i2c_set_pin` 把两脚**交还 I2C 外设**（否则位置留在 GPIO 功能，后续事务全废）。
/// 与启动期版本唯一差别就是第 3 步（启动期驱动尚未装，不需要交还）。
///
/// `force` 参数：**无条件**打 9 时钟 + STOP。真板实锤（2026-10-12 第二批）：舵机电机
/// 换向噪声会在**空闲期**（两笔事务之间）把 SSD1306 的 I2C 状态机顶进"半途态"——此后
/// 恒 NACK，而**此刻 SDA 是高电平**（不是被拉低的"锁死"），故条件版（只在 SDA 低时打
/// 时钟）根本不触发、救不回来。故在每次动作收尾（尤其舵机动作后）**强制**重同步一次。
///
/// **引脚真值 fail-closed（批N4 件七(c)）**：SDA/SCL 号取注入态
/// ([`crate::state::rr3_board_bindings_i2c_pair`]，宿主经 `rr3.set_bindings` 下发）；
/// 未注入即**静默拒**（不碰 GPIO——引脚号未知时任何位打操作都是越权）。运行期每拍
/// 调用方的 `selfheal_skipped` 计数与本路径重试终败的 Err 冒泡各自是观测面。
pub(crate) fn i2c_bus_recover(force: bool) {
    use esp_idf_sys as sys;
    let Some((sda, scl)) = rr3_board_bindings_i2c_pair() else {
        return;
    };
    let (sda, scl) = (sda as i32, scl as i32);
    unsafe {
        sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT);
        sys::gpio_set_pull_mode(sda, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
        sys::gpio_set_direction(scl, sys::gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD);
        sys::gpio_set_pull_mode(scl, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
        sys::gpio_set_level(scl, 1);
        sys::esp_rom_delay_us(5);
        let stuck = sys::gpio_get_level(sda) == 0;
        if force || stuck {
            // SDA 被从机拉低 = 丢拍锁死：9 个时钟走完从机位计数器 + STOP 释放。
            // force 但 SDA 高 = 半途态：同样打 9 时钟 + STOP 把从机推回 idle。
            if stuck {
                println!("[periph] I2C 自愈：SDA 被从机拉低（丢拍锁死），打 9 时钟 + STOP");
            }
            for _ in 0..9 {
                sys::gpio_set_level(scl, 0);
                sys::esp_rom_delay_us(5);
                sys::gpio_set_level(scl, 1);
                sys::esp_rom_delay_us(5);
            }
            sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD);
            sys::gpio_set_level(sda, 0);
            sys::esp_rom_delay_us(5);
            sys::gpio_set_level(scl, 1);
            sys::esp_rom_delay_us(5);
            sys::gpio_set_level(sda, 1);
            sys::esp_rom_delay_us(5);
            sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT);
        }
        // 交还 I2C 外设（IOMUX 复位为 I2C 功能 + 内部上拉），驱动内部状态不受影响
        sys::i2c_set_pin(
            sys::i2c_port_t_I2C_NUM_0,
            sda,
            scl,
            true,
            true,
            sys::i2c_mode_t_I2C_MODE_MASTER,
        );
    }
}

/// I2C 总线解卡（**欠压复位后的自愈**）：ESP32 在 SSD1306 事务中途被 BOD 复位时，
/// 从机常保持 SDA 低电平（半个字节未收完），此后主机每次 START 都撞"总线忙"→
/// 全部事务 NACK/超时（真板实录：复位后 boot 扫描恒 `[]`、init 5 连败）。IC 上电
/// 前先用 GPIO 手动发 9 个 SCL 时钟把从机的位计数器走完、再补一个 STOP，SDA 即
/// 释放。裸寄存器操作（不进 HAL 驱动状态机），随后 I2cDriver::new 会重配这两脚。
///
/// **引脚真值 fail-closed（批N4 件七(c)）**：引脚号取注入态（同
/// [`i2c_bus_recover`]）；未注入回 `false` 拒不动作（boot 期桥未连、注入必缺席
/// ——v3 拍板：消费面级 fail-closed，不做板级等宿主的启动时序重构，启动期解卡
/// 缺席的取舍与点名归调用方），调用方据此打站。
///
/// 返回：`true` = 已注入且解卡动作走完（SDA 高则本就无事可做，同回 true）；
/// `false` = 绑定未注入，拒。
pub(crate) unsafe fn i2c_bus_unstick() -> bool {
    use esp_idf_sys as sys;
    let Some((sda, scl)) = rr3_board_bindings_i2c_pair() else {
        return false;
    };
    let (sda, scl) = (sda as i32, scl as i32);
    // SDA 输入上拉（读电平）、SCL 开漏输出上拉
    sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT);
    sys::gpio_set_pull_mode(sda, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
    sys::gpio_set_direction(scl, sys::gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD);
    sys::gpio_set_pull_mode(scl, sys::gpio_pull_mode_t_GPIO_PULLUP_ONLY);
    sys::gpio_set_level(scl, 1);
    sys::esp_rom_delay_us(5);
    if sys::gpio_get_level(sda) == 0 {
        // SDA 被从机拉低 = 卡死：走 9 个时钟
        for _ in 0..9 {
            sys::gpio_set_level(scl, 0);
            sys::esp_rom_delay_us(5);
            sys::gpio_set_level(scl, 1);
            sys::esp_rom_delay_us(5);
        }
        // 补一个 STOP：SCL 高时 SDA 由低→高
        sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD);
        sys::gpio_set_level(sda, 0);
        sys::esp_rom_delay_us(5);
        sys::gpio_set_level(scl, 1);
        sys::esp_rom_delay_us(5);
        sys::gpio_set_level(sda, 1);
        sys::esp_rom_delay_us(5);
        sys::gpio_set_direction(sda, sys::gpio_mode_t_GPIO_MODE_INPUT);
        println!("[periph] I2C 总线解卡：SDA 曾被拉低，已发 9 时钟 + STOP");
    }
    true
}

/// 发命令（控制字节 0x00 + 命令字节序列）。SSD1306 控制字节 Co=0/D/C#=0 表示
/// 其后全部为命令——流式命令面单笔 I2C 写合法（数据手册 §8.1.5.1；init 全序列
/// 25B 一笔发，b7 真板实锤：8B 缓冲护栏误拒 init 致"命令序列超长"假故障）。
pub(crate) fn oled_cmd(i2c: &mut I2cDriver, cmds: &[u8]) -> Result<(), String> {
    let mut buf = [0u8; 32];
    buf[0] = 0x00;
    if cmds.len() > 31 {
        return Err("命令序列超长".into());
    }
    buf[1..=cmds.len()].copy_from_slice(cmds);
    i2c_write_retry(&mut rr3_esp_i2c(i2c), OLED_ADDR, &buf[..=cmds.len()]).map_err(|e| format!("I2C 命令写失败: {e}"))
}

/// 写一页 GDDRAM（128 列）。**分块写**：每笔 ≤33 字节（1 控制字节 + 32 数据），
/// 真板实录——129B 单笔在跳线/弱上拉面连重试 3 次仍 ESP_FAIL，而短命令笔（≤26B）
/// 恒过：本板 I2C 是上升沿边际，**事务越短越易过**。SSD1306 页寻址
/// 下列指针跨事务自动递增（数据手册 §8.1.5.2 连续数据模式），分块续写等价于单笔。
///
/// **整页重试**（外层）：任一块失败 → [`i2c_write_retry`] 内部已解卡，但丢拍后从机
/// 列指针已错位，故本层**把整页（页地址 + 128B）从确定性起点重写**，至多
/// [`I2C_PAGE_ATTEMPTS`] 次；块间留 [`I2C_CHUNK_GAP_MS`]。
fn oled_page(i2c: &mut I2cDriver, page: u8, data: &[u8; 128]) -> Result<(), String> {
    /// 单笔数据字节上限（32B 为实测经验值：128B 单笔恒 NACK，32B 分块时过时挂）。
    const CHUNK: usize = 32;
    let mut last = String::from("未知");
    for _ in 0..I2C_PAGE_ATTEMPTS {
        match oled_page_once(i2c, page, data, CHUNK) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(format!("（整页重写 {I2C_PAGE_ATTEMPTS} 次后）{last}"))
}

/// 单页单趟写（页地址命令 + 分块数据）。失败即返回，由 [`oled_page`] 决定整页重写。
fn oled_page_once(
    i2c: &mut I2cDriver,
    page: u8,
    data: &[u8; 128],
    chunk: usize,
) -> Result<(), String> {
    oled_cmd(i2c, &rr3_page_cmd(page))?;
    let mut buf = [0u8; 33];
    buf[0] = 0x40;
    let mut off = 0usize;
    while off < data.len() {
        let n = chunk.min(data.len() - off);
        buf[1..1 + n].copy_from_slice(&data[off..off + n]);
        i2c_write_retry(&mut rr3_esp_i2c(i2c), OLED_ADDR, &buf[..1 + n])
            .map_err(|e| format!("I2C 页数据写失败 page={page} off={off} n={n}: {e}"))?;
        off += n;
        if off < data.len() {
            thread::sleep(Duration::from_millis(I2C_CHUNK_GAP_MS));
        }
    }
    Ok(())
}

/// 初始化（页寻址模式 + 电荷泵开 + 全清屏；序列为 SSD1306 数据手册标准序）。
pub(crate) fn oled_init(i2c: &mut I2cDriver) -> Result<(), String> {
    // 初始化序真值在 periph-mech（P0-A A3）；初始亮度取策略面首档（**中档**：
    // 满亮 0xCF 会在胶囊眼大面亮点上屏时反噬 I2C，故起点即已调低，失败再逐级
    // 向下——见 OLED_CONTRAST_LEVELS）。
    oled_cmd(i2c, &rr3_init_commands(OLED_CONTRAST_LEVELS[0]))?;
    oled_clear(i2c)
}

/// 全清屏（8 页 × 128B = 1KB 全零）。
fn oled_clear(i2c: &mut I2cDriver) -> Result<(), String> {
    for page in 0u8..8 {
        oled_page(i2c, page, &RR3_OLED_ZERO_PAGE)?;
    }
    Ok(())
}

/// 画一行文本（row 0/1 ↦ 页 0/2；≤21 可见字符，超出截断——行协议侧同样约束）。
pub(crate) fn oled_text(i2c: &mut I2cDriver, row: usize, text: &[u8]) -> Result<(), String> {
    // 行号/列数策略量按参喂给编码面（P0-A A3）：本档只定页地址与传输。
    let buf = rr3_text_page(row, text, OLED_ROWS, OLED_COLS)?;
    oled_page(i2c, (row * 2) as u8, &buf)
}

/// I2C 总线扫描（0x03–0x77 逐地址单字节 0x00 探测，返回应答地址表）。
/// 判据①：0x3C 必在表内（全案 §4 批B ① 预期输出）。
pub(crate) fn i2c_scan(i2c: &mut impl Rr3I2cPort) -> Vec<u8> {
    let mut found = Vec::new();
    for addr in 0x03u8..=0x77 {
        if i2c.rr3_i2c_write(addr, &[0x00]).is_ok() {
            found.push(addr);
        }
    }
    found
}

/// 8x8 点阵帧 → OLED 中央 64x64 放大渲染：源帧每像素横向铺 8 列、纵向铺 8 行，
/// 落在列 32..96 × 全 8 页；页 p 全部 8 个位行同显源行 p（sy = (p*8+by)/8 = p）。
/// 位序映射：MAX7219 字节 bit7=行最左 ↔ 目标列 x=32 处（c=0 取 bit7）——方向
/// 不镜像；SSD1306 页内列字节 bit0=页顶。两侧留白清零（居中不残影）。
pub(crate) fn oled_frame(i2c: &mut I2cDriver, frame: &[u8; 8]) -> Result<(), String> {
    // 点阵放大编码在 periph-mech（P0-A A3）；本档逐页传输。
    let pages = rr3_frame_pages(frame);
    for (page, buf) in pages.iter().enumerate() {
        oled_page(i2c, page as u8, buf)?;
    }
    Ok(())
}

/// 胶囊眼渲染（**I2C 传输薄壳**）：位图几何在 face 机制件（atoms/face/mech.rs，
/// 批N23 件二收编），本函数逐页取图后只做页写——传输面与面目语义分离。
pub(crate) fn oled_capsules(i2c: &mut I2cDriver, style: EyeStyle) -> Result<(), String> {
    for page in 0u8..8 {
        let buf = eye_page(page, style);
        oled_page(i2c, page, &buf)?;
    }
    Ok(())
}
