"""喊它要有回应：声音事件 → 表情/屏显/转向 的实机闭环。

用法: python3 voice_respond.py [termios 常量名，缺省 B1500000] [秒数，缺省 90]
      换口用 env `RR3_PROBE_TTY`。

**诚实口径**：当前回应由**声音事件**（RMS 超环境底噪）触发，不是关键词识别——
"你好洛克"的 ASR/KWS 未建（记账遗留）；喊任何响声都会触发。

闭环（全在宿主侧，板固件零改动）：
  AUDIO ON → 流式收 Event 帧 → 解码 PCM → 逐窗（4 片 ≈512ms）双通道 RMS →
  超阈值（环境底噪 ×1.25，滚动跟踪）→ 回应：FACE happy + TILT 点头（低头→
  抬头→回中）+ PAN 按 ILD 偏置带判向偏转 20°（带内方向不明则回中不冒领）→
  冷却 3s；退场发 FACE idle 回中性。回应**不发 OLED 文本**——OLED 会关掉
  表情模式把脸盖掉（实测踩坑）。

判据：每次喊话在 2s 内看到 FACE happy + TILT + PAN 三串 OK 回包，
`回应 N 次` 计数；触发明细带时间戳与 RMS/ild/判定侧向，供核对
"哪一声对应哪次回应"。
"""
import base64
import json
import math
import os
import statistics
import sys
import time

from probe_common import (
    HEADER_LEN, MAGIC, OFF_PAYLOAD_LEN, baud_from_argv_or_env, flush_input,
    open_tty, pump, read_avail,
)

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
BAUD = baud_from_argv_or_env(1)
SECS = int(sys.argv[2]) if len(sys.argv) > 2 else 90

#: 触发阈值 = 环境底噪 RMS × 此倍数（自适应；固定阈值会随装配/环境漂移失效）。
#: 1.25 按实测定：底噪窗 1024 时喊话窗 1264/1400——1.45 会漏（实测漏 2 次）
TRIGGER_RATIO = 1.25
#: 窗长（分片数）：4 × 128ms ≈ 512ms，够短保响应、够长稳 RMS
WIN_CHUNKS = 4
#: 触发后冷却秒数（防一声喊连发多次回应）
COOLDOWN = 3.0
#: 转向判据：**ILD 偏置带**（判向算法第三代，前两代都有实机判死证据）：
#: ① v5 纯 ILD 符号（±0.5° 死区）——人声 ILD 只有 1-3dB，8 次触发全挤 ±1°
#:    与实际站位对不上，弃；
#: ② v6 TDOA——4.5cm 麦间距 @16kHz 物理上限只有 ±2.1 采样点，实测互相关峰
#:    声程 10.6/-17.1cm 超上限、|c|≈0.04-0.07 噪声主导，判死
#:    （doa_probe.py，53896ce），弃；
#: ③ 本版：AB 定向喊话标定（2026-10-30 实机：用户左手边 onset ILD −2.52dB，
#:    右手边 −0.89/-0.08dB）——两簇**同负**，中点 ≈−1.7dB：阵列存在 ~1dB 的
#:    定向偏置（方向性遮挡；底噪漫射场标定只有 ±0.04dB，测不到它），不能拿
#:    0 当分界。判据改偏置带：ILD ≤ LEFT 判用户左侧、≥ RIGHT 判用户右侧、
#:    带内方向不明 → 回中+点头（不冒领）。ILD 取 chunk 内最响 128ms 窗
#:    （喊话核 SNR 最高；512ms 全窗被喊前静音稀释会趋向 0）。
#:    PAN 映射同日实测钉死（用户目视）：PAN 70 = 转向用户左，PAN 110 = 用户右。
ILD_LEFT_THR = -1.2   # dB，≤ 此值 → 用户左侧
ILD_RIGHT_THR = -0.6  # dB，≥ 此值 → 用户右侧
PAN_DEFLECT = 20
PAN_CENTER = 90
#: 俯且回应动作（用户要求"抬头低头都要有"）：点头示意 = 行程下限→上限→回中
#: （tilt 行程 [80,100]，中位 93；纯示意动作，不携带方向信息——双麦水平排列
#: 分不出垂直方位，诚实口径）
TILT_NOD = [80, 100, 93]

fd = open_tty(DEV, BAUD)
flush_input(fd)
pump(fd, 1.0)
parse_errors = 0


def line(cmd, wait=1.0):
    """发一条行面命令（带空闲前导，与生产宿主同口径）并收回包文本。"""
    os.write(fd, b"\n")
    time.sleep(0.01)
    os.write(fd, (cmd + "\n").encode())
    return pump(fd, wait).decode("utf-8", "replace")


def parse_pcm_stream(buf):
    """流式解析：从 `buf`（bytearray，**跨读取块持久**）里就地消费完整帧，
    返回本段新增 PCM 样本对 [(l,r), ...]；不完整的尾帧**保留**在缓冲里等后续
    字节。首版 bug 教训：帧 11KB 而单次 read 只到 ~4KB，按"每次清缓冲再整帧
    解析"则每一帧都跨块、永远凑不齐——必须游标推进 + 残尾保留。
    `parse_errors`（模块级）计数解析失败——异常必须可见，不许裸吞。"""
    global parse_errors
    out = []
    n = len(buf)
    i = buf.find(MAGIC)
    while i >= 0:
        if i + HEADER_LEN > n:
            break  # 头不完整：留到下一轮
        plen = int.from_bytes(buf[i + OFF_PAYLOAD_LEN:i + OFF_PAYLOAD_LEN + 4], "little")
        if plen == 0 or plen > 20000:
            i = buf.find(MAGIC, i + 1)  # 坏长度：跳过这个伪 magic
            continue
        if i + HEADER_LEN + plen > n:
            break  # 载荷不完整：留到下一轮
        body = buf[i + HEADER_LEN:i + HEADER_LEN + plen]
        try:
            v = json.loads(bytes(body[2:]))
            s = v["pcm_b64"]
            # s 是 str（json 解出）——拼接 padding 前必须 encode；str+bytes 的
            # TypeError 曾被裸 except 吞掉导致全程 0 样本（fail-visible 教训）
            pcm = base64.b64decode(s.encode() + b"=" * ((4 - len(s) % 4) % 4))
            ints = list(memoryview(pcm).cast("h"))
            out.extend(zip(ints[0::2], ints[1::2]))
        except Exception as e:
            if parse_errors == 0:
                print(f"  !! 首个解析失败（后续静默计数）: {e!r}")
            parse_errors += 1
        i = i + HEADER_LEN + plen  # 下一帧紧跟（板上背靠背）
    if i > 0:
        del buf[:i]  # 消费掉已解析/已跳过的前缀，残尾（半帧）留队
    return out


def loud_window_ild(chunk, win=2048, hop=512):
    """chunk（8192 样本对 ≈512ms）内最响 128ms 窗的 ILD（dB，L 相对 R）。

    取最响窗而非全窗：喊话核 SNR 最高、被环境混响稀释最少；全窗含喊前静音，
    ILD 会被稀释趋向 0（实测全窗 ILD 比起始/最响窗弱 0.5-0.8dB）。窗长
    2048 点 =128ms：短于语音音节核、足够长稳住 RMS。判读阈值见
    ILD_LEFT_THR 注（偏置带，分界不在 0）。
    """
    mags = [abs(l) + abs(r) for l, r in chunk]
    best_i, best_v = 0, -1.0
    for i in range(0, len(chunk) - win + 1, hop):
        v = sum(mags[i:i + win]) / win
        if v > best_v:
            best_v, best_i = v, i
    seg = chunk[best_i:best_i + win]
    lr = (sum(l * l for l, _ in seg) / win) ** 0.5
    rr = (sum(r * r for _, r in seg) / win) ** 0.5
    return 20 * math.log10(max(lr, 1) / max(rr, 1))


print("=== 启动音频（先关再开，杜绝上一会话遗留流）===")
# 清屏：屏幕上若有旧版写入的 HELLO 文字（OLED 持久显示，v2 遗留），FACE 命令
# 会重开表情模式把它盖掉——不然后续每轮验证用户都看着"旧画面"误判系统没变
print("清屏回应:", line("FACE idle", 0.8).strip()[:60])
line("AUDIO OFF", 0.5)
flush_input(fd)
line("AUDIO ON", 0.8)
buf = bytearray()
t0 = time.time()
while time.time() - t0 < 2.0:
    buf += read_avail(fd, 0.05)
samples = parse_pcm_stream(buf)
mags = [abs(l) + abs(r) for l, r in samples]
ambient = statistics.median(mags) if mags else 0
thr = max(ambient * TRIGGER_RATIO, ambient + 200)
print(f"底噪 |L|+|R| median={ambient}（{len(samples)} 样本对）→ 初始阈值={thr:.0f}")
if not samples:
    print("!! 校准期 0 帧：AUDIO ON 未生效或链路无音频，中止（先补 AUDIO OFF 防遗留）")
    line("AUDIO OFF", 0.8)
    os.close(fd)
    sys.exit(2)

print(f"=== 倾听 {SECS}s：请喊「你好洛克」===")
responses = 0
buf = bytearray()
win = []
t0 = time.time()
cooldown_until = 0.0
last_report = t0
#: 窗满样本对数：16000Hz × 0.512s（4 片）× 2 通道 = 16384 个 i16 = 8192 对
NEED = 8192
while time.time() - t0 < SECS:
    buf += read_avail(fd, 0.05)
    win.extend(parse_pcm_stream(buf))
    # 窗满（≈512ms）即评一次
    if len(win) >= NEED:
        chunk, win = win[:NEED], win[NEED:]
        mags = [abs(l) + abs(r) for l, r in chunk]
        level = statistics.median(mags)
        now = time.time()
        if now - last_report >= 5.0:
            print(f"  [t={now-t0:5.1f}s] 窗电平 {level:.0f}（阈值 {thr:.0f}）")
            last_report = now
        if level >= thr and now >= cooldown_until:
            l_rms = (sum(l * l for l, _ in chunk) / len(chunk)) ** 0.5
            r_rms = (sum(r * r for _, r in chunk) / len(chunk)) ** 0.5
            # 判向：最响窗 ILD 落偏置带（阈值证据见 ILD_LEFT_THR 注）
            ild = loud_window_ild(chunk)
            if ild <= ILD_LEFT_THR:
                pan, where = PAN_CENTER - PAN_DEFLECT, "左"
            elif ild >= ILD_RIGHT_THR:
                pan, where = PAN_CENTER + PAN_DEFLECT, "右"
            else:
                pan, where = PAN_CENTER, "不明"
            responses += 1
            # 回应 = 大动作序列（每个动作都要**目视可辨**——v4 的 PAN 20° 静音
            # 微调 + FACE talk 微动作被实测判"没反应"）：
            #   表情切换 → TILT 点头（低头→抬头→回中）→ PAN 转向声源侧
            print(f"  ★ 听到了 #{responses}（t={now-t0:.1f}s RMS L={l_rms:.0f}/R={r_rms:.0f} ild={ild:+.2f}dB→{where}）→ 回应：FACE happy + 点头 + PAN {pan}")
            print("    ", line("FACE happy", 0.6).strip()[:60])
            for t in TILT_NOD:
                print(f"    TILT {t}:", line(f"TILT {t}", 1.2).strip()[:60])
            print(f"    PAN {pan}:", line(f"PAN {pan}", 1.5).strip()[:60])
            cooldown_until = time.time() + COOLDOWN
        # ---- 滚动底噪跟踪（单次校准对瞬态噪声零鲁棒性的根治）----
        # 下探即时（安静窗立即把底噪拉回真实值）；上探每窗最多 +0.5%（语音爆发
        # 512ms 内底噪几乎不动，不会被自己的喊话抬走）。实测教训：启动校准窗
        # 恰逢遗留流/瞬态噪声 → 底噪被抬到 1816（真实 1024）→ 阈值 2270 → 全程漏检。
        if level < ambient:
            ambient = level
        else:
            ambient = min(ambient * 1.005, level)
        thr = max(ambient * TRIGGER_RATIO, ambient + 200)

line("AUDIO OFF", 1.0)
line("FACE idle", 0.8)  # 退场回中性表情：不给板子留下 happy 定格/文本模式残留
print(f"\n回应 {responses} 次 / 监听 {SECS}s ｜ 解析失败 {parse_errors} 帧")
os.close(fd)
sys.exit(0 if responses > 0 else 1)
