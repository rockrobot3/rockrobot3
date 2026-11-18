"""喊它要有回应：声音事件 → 表情/屏显/转向 的实机闭环。

用法: python3 voice_respond.py [termios 常量名，缺省 B1500000] [秒数，缺省 90]
      换口用 env `RR3_PROBE_TTY`。

**诚实口径**：当前回应由**声音事件**（RMS 超环境底噪）触发，不是关键词识别——
"你好洛克"的 ASR/KWS 未建（记账遗留）；喊任何响声都会触发。

闭环（全在宿主侧，板固件零改动）：
  AUDIO ON → 流式收 Event 帧 → 解码 PCM → 512ms 评估粒度内滑 128ms 窗取
  最响窗联合 RMS → 超阈值（底噪 ×1.45，滚动跟踪）→ 回应：FACE happy + TILT
  点头（低头→抬头→回中）+ PAN 回中 → 冷却 3s；退场发 FACE idle 回中性。
  **方向声明在锚点重建前撤除**（v7.2，依据见 PAN_CENTER 注）——每声照记
  ILD 进账簿。回应**不发 OLED 文本**——OLED 会关掉表情模式把脸盖掉（实测踩坑）。

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

#: 触发与判向统计量（v7.1；统计量经三代实测淘汰）：
#: ① v6 及以前用 512ms 窗 |l|+|r| 中位——静室稳（1024±1）但对 ~1m 定向喊话只抬
#:    1.1-1.2×，实测漏检（2026-10-30 三次定向采集全漏）；
#: ② 16ms 窗最大值——抓喊话核灵敏，但深夜静室噪声自身就冲 1.7×（实测 1740/
#:    1024，~300 窗的极值分布），1.35× 阈值会常误触，弃；
#: ③ 本版 **128ms 窗联合 RMS**（sqrt(mean(l²+r²))）：噪声窗 RMS 波动仅 ±2%
#:    （2048 样本自由度），近距喊话实测 2.1×、1m 喊话 1.3-1.6×——TRIG_RATIO=
#:    1.45 两侧都分得开。触发窗即判向窗（喊话核 SNR 最高段）。
TRIG_RATIO = 1.45
#: 窗长（分片数）：4 × 128ms ≈ 512ms 评估粒度（响应延迟上限）
WIN_CHUNKS = 4
#: 窗满样本对数：16000Hz × 0.512s（4 片）× 2 通道 = 16384 个 i16 = 8192 对
NEED = 8192
#: 触发后冷却秒数（防一声喊连发多次回应）
COOLDOWN = 3.0
#: 判向：**v7.2 起方向声明全部撤除**（回应=表情+点头+回中）。撤除依据
#: （2026-10-30 深夜离线回放复盘，全程可复核）：
#: ① 今天三次"定向采集"（L/R/严格R）的 onset 计量 1241-1340，**全部低于深夜
#:    静室纯噪声的 16ms 窗最大值 1740**——即从未录到真实喊声，此前的"左锚点
#:    −2.52dB / 右锚点 −0.89dB"是噪声涨落，作废；
#: ② 全部可信喊声样本只有正前方 r5/r6 共 6 次触发，ILD ∈ [−0.16,+0.06]dB
#:    （双麦对称锚）；其中 1 次（r6 尾段，ratio=1.53）ILD=−2.31dB——正前喊声
#:    的混响尾即可产生假左签名，按 ILD 转左会误转；
#: ③ 判向算法前几代判死记录：纯符号死区（8 次触发全挤 ±1°）、TDOA（4.5cm
#:    麦距@16kHz 物理上限 ±2.1 采样点，实测峰值声程超上限，doa_probe
#:    53896ce）、ILD 三档带（右档与正前区间重叠）。
#: 恢复转向的条件：白天用本版触发器（能真正录到喊声）采集带真值标注的定向
#: 喊话，重建锚点后再开。另注意：0.4m 喊话对底噪能量比仅 ~5dB，物理预期
#: ~40dB——疑阵列声学衰减 ~30dB（壳体遮挡？），白班物理检查项（status.md）。
PAN_CENTER = 90
#: 俯且回应动作（用户要求"抬头低头都要有"）：点头示意 = 行程下限→上限→回中
#: （tilt 行程 [80,100]，中位 93；纯示意动作，不携带方向信息——双麦水平排列
#: 分不出垂直方位，诚实口径）
TILT_NOD = [80, 100, 93]

parse_errors = 0


def line(fd, cmd, wait=1.0):
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


def window_stats(chunk, off, win=2048):
    """chunk[off:off+win]（128ms）的联合 RMS 与 ILD。

    联合 RMS = sqrt(mean(l²+r²))——双通道能量合并，对「哪边响」不敏感、对
    「有没有声」最稳（噪声窗波动 ±2%）；ILD = 20log10(rmsL/rmsR)，判读阈值见
    ILD_LEFT_THR 注。win=2048 点 =128ms：短于语音音节核、足够长压住噪声起伏。
    """
    seg = chunk[off:off + win]
    sl = sum(l * l for l, _ in seg)
    sr = sum(r * r for _, r in seg)
    lr = (sl / win) ** 0.5
    rr = (sr / win) ** 0.5
    level = ((sl + sr) / (2 * win)) ** 0.5
    return level, 20 * math.log10(max(lr, 1) / max(rr, 1))


def loudest_window(chunk, win=2048, hop=512):
    """chunk（8192 对 ≈512ms）内最响 128ms 窗的 (level, ild)。

    取最响窗而非全窗：喊话核 SNR 最高、被环境混响稀释最少；512ms 全窗含喊前
    静音，统计量被稀释趋向底噪（实测 1m 喊话全窗只抬 1.1-1.2×，v6 漏检根因）。
    """
    best = None
    for off in range(0, len(chunk) - win + 1, hop):
        st = window_stats(chunk, off, win)
        if best is None or st[0] > best[0]:
            best = st
    return best


def quietest_window(chunk, win=2048, hop=512):
    """chunk 内最安静 128ms 窗的联合 RMS——本分片的环境底噪估计（滚动跟踪用）。"""
    return min(window_stats(chunk, off, win)[0]
               for off in range(0, len(chunk) - win + 1, hop))


def main():
    fd = open_tty(DEV, BAUD)
    flush_input(fd)
    pump(fd, 1.0)
    print("=== 启动音频（先关再开，杜绝上一会话遗留流）===")
    # 清屏：屏幕上若有旧版写入的文字（OLED 持久显示），FACE 命令会重开表情
    # 模式把它盖掉——不然后续每轮验证用户都看着"旧画面"误判系统没变
    print("清屏回应:", line(fd, "FACE idle", 0.8).strip()[:60])
    line(fd, "AUDIO OFF", 0.5)
    flush_input(fd)
    line(fd, "AUDIO ON", 0.8)
    buf = bytearray()
    t0 = time.time()
    while time.time() - t0 < 2.0:
        buf += read_avail(fd, 0.05)
    samples = parse_pcm_stream(buf)
    if len(samples) < 4096:
        print(f"!! 校准期样本不足（{len(samples)} 对）：AUDIO ON 未生效或链路无音频，"
              "中止（先补 AUDIO OFF 防遗留）")
        line(fd, "AUDIO OFF", 0.8)
        os.close(fd)
        sys.exit(2)
    floor128 = statistics.median(
        window_stats(samples, off)[0]
        for off in range(0, len(samples) - 2048 + 1, 512)
    )
    print(f"底噪 128ms 联合 RMS median={floor128:.0f}（{len(samples)} 样本对）→ "
          f"触发阈值={floor128 * TRIG_RATIO:.0f}")

    print(f"=== 倾听 {SECS}s：请喊「你好洛克」===")
    responses = 0
    buf = bytearray()
    win = []
    t0 = time.time()
    cooldown_until = 0.0
    last_report = t0
    while time.time() - t0 < SECS:
        buf += read_avail(fd, 0.05)
        win.extend(parse_pcm_stream(buf))
        # 窗满（≈512ms）即评一次
        if len(win) >= NEED:
            chunk, win = win[:NEED], win[NEED:]
            level, ild = loudest_window(chunk)
            now = time.time()
            if now - last_report >= 5.0:
                print(f"  [t={now-t0:5.1f}s] 底噪 {floor128:.0f} 最响128ms窗 "
                      f"{level:.0f}（阈值 {floor128 * TRIG_RATIO:.0f}）")
                last_report = now
            if level >= floor128 * TRIG_RATIO and now >= cooldown_until:
                # v7.2：方向声明撤除期——回应不转向，只回中；ILD 照打印进账簿，
                # 供白天带真值标注的定向采集重建锚点（依据见 PAN_CENTER 注）
                pan, where = PAN_CENTER, "不判向(待标定)"
                responses += 1
                # 回应 = 大动作序列（每个动作都要**目视可辨**——v4 的 PAN 20° 静音
                # 微调 + FACE talk 微动作被实测判"没反应"）：
                #   表情切换 → TILT 点头（低头→抬头→回中）→ PAN 回中
                print(f"  ★ 听到了 #{responses}（t={now-t0:.1f}s 128ms窗RMS "
                      f"{level:.0f}/{floor128:.0f}={level / floor128:.2f}× "
                      f"ild={ild:+.2f}dB→{where}）→ 回应：FACE happy + 点头 + PAN {pan}")
                print("    ", line(fd, "FACE happy", 0.6).strip()[:60])
                for t in TILT_NOD:
                    print(f"    TILT {t}:", line(fd, f"TILT {t}", 1.2).strip()[:60])
                print(f"    PAN {pan}:", line(fd, f"PAN {pan}", 1.5).strip()[:60])
                cooldown_until = time.time() + COOLDOWN
            # ---- 滚动底噪跟踪（与触发同统计量）----
            # 估计值 = 本分片最安静 128ms 窗：下探即时、上探每窗最多 +0.5%（语音
            # 爆发期底噪几乎不动，不会被自己的喊话抬走）。实测教训：启动校准窗恰逢
            # 遗留流/瞬态噪声 → 底噪被抬上天 → 全程漏检；滚动下探根治。
            quiet = quietest_window(chunk)
            if quiet < floor128:
                floor128 = quiet
            else:
                floor128 = min(floor128 * 1.005, quiet)

    line(fd, "AUDIO OFF", 1.0)
    line(fd, "FACE idle", 0.8)  # 退场回中性表情：不给板子留下 happy 定格/文本模式残留
    print(f"\n回应 {responses} 次 / 监听 {SECS}s ｜ 解析失败 {parse_errors} 帧")
    os.close(fd)
    sys.exit(0 if responses > 0 else 1)


if __name__ == "__main__":
    main()
