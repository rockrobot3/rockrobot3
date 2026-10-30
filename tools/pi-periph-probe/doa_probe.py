"""DoA 判向判定实验——先测物理证据，再定判向算法（测量先行，不给结论留假设）。

背景（为什么不是直接改 voice_respond）：v5 TDOA 实机 3 次触发 lag 全负
（-1/-3/-4），而 ~4.5cm 麦间距在 16kHz（采样点 = 2.14cm 声程）下物理最大
只有 ±2.1 采样点——观测值超出物理上限，峰值必是噪声主导，判向不可信。
ILD 也早已实测失效（8 次触发 az 全挤 ±1°）。两个候选特征都缺一份**带真值
标注**的原始证据，改算法就是盲改。

本探针做什么：六轮带 OLED 屏幕提示的定向喊话（左×2 / 右×2 / 中×2），每轮
AUDIO ON 采集 → AUDIO OFF（流停、读回干净，杜绝 PCM 帧污染回包读数），对
每轮提取三种证据并打印：

  1. 起始段 ILD（onset 后 64ms，减底噪 L/R 增益偏差）——v5 三次触发 L 恒比
     R 高 1-2%，疑似固定通道偏差，不标定则 ILD 永远偏向一侧；
  2. 全窗 ILD（onset 后 256ms，同样校正）；
  3. 互相关剖面（onset+16ms 起 128ms 窗，差分预白化，±8 lag，抛物线插值出
     分数 lag 与物理声程）——同时报 argmax / argmin / argmax|c| 三口径：
     v5 用 abs(c) 选峰是疑点（真峰应为正相关，负峰是反相关伪峰）。

末尾输出各特征与真值方位的一致率表，同时验两个假设：H1 = 通道 L 的麦在
物理左侧（正证据→判左）、H2 = 镜像。映射与判别力由这份证据钉死，之后才改
voice_respond 的判向逻辑。

用法（Pi 上）::

    python3 -u doa_probe.py B1500000 [每轮采集秒=6] [轮数上限=6]

每轮开始前 OLED 显示 `R1:L` 这类提示——看着机器人屏幕，屏幕亮出哪轮就从
哪个方位喊两声「你好洛克」。原始 PCM 落 /tmp/doa_r{N}.pcm 供复核。
"""
import base64
import json
import math
import os
import statistics
import sys
import time

from probe_common import (
    MAGIC, HEADER_LEN, OFF_PAYLOAD_LEN,
    baud_from_argv_or_env, open_tty, flush_input, read_avail,
    drain_until_quiet,
)

DEV = os.environ.get("RR3_PROBE_TTY", "/dev/ttyAMA0")
BAUD = baud_from_argv_or_env(1)
#: 单侧快跑模式：`doa_probe.py B1500000 L 12` = 只测左一轮 12s。聊天里约定
#: "启动后 12s 内任意时刻喊两声"即可——六轮连跑靠用户盯屏幕对节奏太脆（三轮
#: 实验只拿到 2-5/6 有效触发）；标定跳过（三轮实测通道偏差 ≤0.04dB，可忽略）。
#: `AB` = 双相位 A/B：先用户左手边 12s、隔 8s 再右手边 12s——两段 ILD 符号
#: 一锤定音钉死「用户方位→通道」映射（此前 L/R 轮各自为负，疑似参照系混淆）。
FAST = len(sys.argv) > 2 and sys.argv[2] in ("L", "R", "C", "AB")
if FAST:
    ROUNDS = [sys.argv[2]] if sys.argv[2] != "AB" else ["L", "R"]
    SHOT_SECS = float(sys.argv[3]) if len(sys.argv) > 3 else 12.0
else:
    SHOT_SECS = float(sys.argv[2]) if len(sys.argv) > 2 else 6.0
    #: 轮次计划（真值表）：左×2 → 右×2 → 中×2。中轮是阴性对照：两特征都应≈0。
    ROUNDS = ["L", "L", "R", "R", "C", "C"]
#: AB 双相位间的换位间隔秒。
AB_GAP_SECS = 8.0
#: 1 采样点 @16kHz 的声程（343 m/s / 16000）。互相关 lag 换算物理量用。
CM_PER_SAMPLE = 34300.0 / 16000.0
#: onset 检出：轮内全轮中位 × 1.2（v5 实测喊话窗只有 1.25-1.4× 中位，1.3 会漏）
ONSET_RATIO = 1.2

parse_errors = 0


def line(fd, cmd, wait=0.8):
    """发一条行面命令（带空闲前导，与生产宿主同口径）并收回包文本。"""
    os.write(fd, b"\n")
    time.sleep(0.01)
    os.write(fd, (cmd + "\n").encode())
    return pump_short(fd, wait)


def pump_short(fd, sec):
    out = b""
    t0 = time.time()
    while time.time() - t0 < sec:
        d = read_avail(fd, 0.05)
        if d:
            out += d
    return out


def parse_pcm_stream(buf):
    """流式解析（与 voice_respond 同一份逻辑的独立拷贝——那边是未提交的在改
    件，不 import 免拖进它的模块级实验代码）：游标推进 + 残尾保留。附加守卫：
    缓冲里 64KB 仍无 magic 就清到只剩尾部 3 字节（防非帧文本撑爆内存；3 字节
    恰好覆盖跨块 torn magic）。"""
    global parse_errors
    out = []
    n = len(buf)
    i = buf.find(MAGIC)
    while i >= 0:
        if i + HEADER_LEN > n:
            break
        plen = int.from_bytes(buf[i + OFF_PAYLOAD_LEN:i + OFF_PAYLOAD_LEN + 4], "little")
        if plen == 0 or plen > 20000:
            i = buf.find(MAGIC, i + 1)
            continue
        if i + HEADER_LEN + plen > n:
            break
        body = buf[i + HEADER_LEN:i + HEADER_LEN + plen]
        try:
            v = json.loads(bytes(body[2:]))
            s = v["pcm_b64"]
            pcm = base64.b64decode(s.encode() + b"=" * ((4 - len(s) % 4) % 4))
            ints = memoryview(pcm).cast("h")
            out.extend(zip(ints[0::2], ints[1::2]))
        except Exception as e:
            if parse_errors == 0:
                print(f"  !! 首个解析失败（后续静默计数）: {e!r}")
            parse_errors += 1
        i = i + HEADER_LEN + plen
    if i > 0:
        del buf[:i]
    elif i < 0 and len(buf) > 65536:
        del buf[:-3]
    return out


def collect(fd, secs):
    """AUDIO ON 采 `secs` 秒流式 PCM，返回样本对列表（本函数只管收不管开/关）。"""
    buf = bytearray()
    pairs = []
    t0 = time.time()
    while time.time() - t0 < secs:
        d = read_avail(fd, 0.05)
        if d:
            buf.extend(d)
            pairs.extend(parse_pcm_stream(buf))
    return pairs


def rms(xs):
    if not xs:
        return 0.0
    s = 0
    for v in xs:
        s += v * v
    return math.sqrt(s / len(xs))


def db(x):
    return 20.0 * math.log10(max(x, 1e-9))


def onset_index(pairs, win=256, hop=128):
    """轮内基线（**全轮** mean|l|+|r| 中位——不用前 1s：喊话若来得早会混进基线
    把阈值抬高导致漏检；喊话占轮时长 <30%，全轮中位天然排除）× ONSET_RATIO
    为阈值，滑窗找首个超阈值窗，返回样本索引；无触发返回 None（v 附带全程
    最大比值，区分「喊了没过线」与「没喊」）。"""
    n = len(pairs)
    vals = [
        (i, sum(abs(pairs[j][0]) + abs(pairs[j][1]) for j in range(i, i + win)) / win)
        for i in range(0, n - win + 1, win)
    ]
    base = statistics.median(v for _, v in vals) if vals else 0.0
    thr = base * ONSET_RATIO
    vmax, imax = 0.0, 0
    for i in range(0, n - win + 1, hop):
        v = sum(abs(pairs[j][0]) + abs(pairs[j][1]) for j in range(i, i + win)) / win
        if v > vmax:
            vmax, imax = v, i
        if v > thr:
            return i, base, v, imax
    return None, base, vmax / base if base > 0 else 0.0, imax


def xcorr_profile(L, R, start, nwin=2048, max_lag=8):
    """差分预白化 + 时域互相关剖面。返回 (lags, 归一化 c, argmax, argmin, argmax|c|, 分数lag)。

    与 voice_respond.tdoa_lag 的两处方法学差异（都在验它的疑点）：
    a) 同时报 argmax(c)（真峰口径）与 argmax|c|（v5 口径）——abs 选峰会把
       反相关负伪峰当成峰；
    b) 抛物线插值出分数 lag——16kHz 下 1 采样点 = 2.14cm，整点量化太粗。
    """
    end = start + nwin
    dl = [L[i] - L[i - 1] for i in range(start + 1, end)]
    dr = [R[i] - R[i - 1] for i in range(start + 1, end)]
    n = len(dl)
    el = math.sqrt(sum(v * v for v in dl))
    er = math.sqrt(sum(v * v for v in dr))
    norm = el * er if el > 0 and er > 0 else 1.0
    lags = list(range(-max_lag, max_lag + 1))
    cs = []
    for lag in lags:
        s = 0
        for i in range(n):
            j = i + lag
            if 0 <= j < n:
                s += dl[i] * dr[j]
        cs.append(s / norm)
    kmax = max(range(len(cs)), key=lambda k: cs[k])
    kmin = min(range(len(cs)), key=lambda k: cs[k])
    kabs = max(range(len(cs)), key=lambda k: abs(cs[k]))
    k = kabs if abs(cs[kabs]) >= cs[kmax] else kmax  # 展示口径为主
    frac = float(lags[k])
    if 0 < k < len(cs) - 1:
        den = cs[k - 1] - 2 * cs[k] + cs[k + 1]
        if den < 0:  # 峰才可插值（谷插值无意义）
            frac = lags[k] + 0.5 * (cs[k - 1] - cs[k + 1]) / den
    return lags, cs, lags[kmax], lags[kmin], lags[kabs], frac


def main():
    fd = open_tty(DEV, BAUD)
    drain_until_quiet(fd, 0.5, 2.0)
    mode = f"快跑单侧 {ROUNDS}" if FAST else f"{len(ROUNDS)} 轮（计划 {'→'.join(ROUNDS)}）"
    print(f"=== DoA 判定实验：{mode}，每轮 {SHOT_SECS}s ===")
    line(fd, "AUDIO OFF", 0.5)  # 杜绝上一会话遗留流
    if FAST:
        bias_db = 0.0  # 三轮标定实测 ≤0.04dB，快跑跳过（见 FAST 注）
    else:
        line(fd, "FACE idle", 0.5)  # 清掉旧 OLED 文字，进表情模式
        # ---- 底噪标定：通道 L/R 增益偏差（v5 观测 L 恒高 1-2% 的量刑定准）----
        print("=== 底噪标定（3s 静默）===")
        line(fd, "AUDIO ON", 0.5)
        noise = collect(fd, 3.0)
        line(fd, "AUDIO OFF", 0.8)
        drain_until_quiet(fd, 0.4, 1.5)
        ambL = rms([p[0] for p in noise])
        ambR = rms([p[1] for p in noise])
        bias_db = db(ambL / ambR) if ambR > 0 else 0.0
        print(f"  底噪 {len(noise)} 对：rms L={ambL:.0f} R={ambR:.0f} → 通道偏差 {bias_db:+.2f} dB（ILD 判读一律减它）")

    rows = []
    for idx, side in enumerate(ROUNDS, 1):
        if FAST:
            print(f"--- 第 {idx} 轮：你的【{'左' if side == 'L' else '右'}】手边"
                  f"（12s 内任意时刻喊两声）---")
        else:
            print(f"--- 第 {idx} 轮：真值方位 = {side}（OLED 已亮提示，请喊两声）---")
        if not FAST:
            line(fd, f"OLED R{idx}:{side}", 0.6)
        elif len(ROUNDS) > 1 and idx > 1:
            print(f"  （{AB_GAP_SECS:.0f}s 换位：请走到你的【右】手边）")
            time.sleep(AB_GAP_SECS)
        flush_input(fd)
        line(fd, "AUDIO ON", 0.5)
        pairs = collect(fd, SHOT_SECS)
        line(fd, "AUDIO OFF", 0.8)
        drain_until_quiet(fd, 0.4, 1.5)
        with open(f"/tmp/doa_r{idx}.pcm", "wb") as f:
            f.write(b"".join(
                p[0].to_bytes(2, "little", signed=True) + p[1].to_bytes(2, "little", signed=True)
                for p in pairs
            ))
        n = len(pairs)
        if n < 8192:
            print(f"  !! 样本不足（{n} 对），本轮作废")
            rows.append((side, None, None, None, None, None))
            continue
        L = [p[0] for p in pairs]
        R = [p[1] for p in pairs]
        on, base, v, imax = onset_index(pairs)
        if on is None:
            print(f"  （无触发：基线 {base:.0f}，全程最大比值 {v:.2f}×——"
                  f">1.15 说明喊了但没过线，<1.1 说明这轮没收到声音）")
            rows.append((side, None, None, None, None, None))
            continue
        # 起始段 ILD：onset 后 64ms（1024 采样），减通道偏差
        seg = 1024
        ild_on = db(rms(L[on:on + seg])) - db(rms(R[on:on + seg])) - bias_db
        # 全窗 ILD：onset 后 256ms
        seg2 = min(4096, n - on)
        ild_full = db(rms(L[on:on + seg2])) - db(rms(R[on:on + seg2])) - bias_db
        # 最响窗 ILD（onset 可能抓到走动声；最响窗更可能是真喊声峰值段）
        m2 = min(imax + 4096, n)
        ild_max = db(rms(L[imax:m2])) - db(rms(R[imax:m2])) - bias_db
        # 互相关：跳过起始 16ms（上升沿混响未稳），128ms 窗
        lags, cs, kmax, kmin, kabs, frac = xcorr_profile(L, R, on + 256)
        print(f"  onset@{on / 16000.0:.2f}s 基线={base:.0f} 触发值={v:.0f} 最响窗@{imax / 16000.0:.2f}s")
        print(f"  起始ILD={ild_on:+.2f}dB 全窗ILD={ild_full:+.2f}dB 最响窗ILD={ild_max:+.2f}dB")
        print(f"  xcorr: argmax={kmax:+d}({cs[lags.index(kmax)]:+.3f}) "
              f"argmin={kmin:+d}({cs[lags.index(kmin)]:+.3f}) "
              f"argmax|c|={kabs:+d} |c|={abs(cs[lags.index(kabs)]):.3f} frac={frac:+.2f} "
              f"声程={frac * CM_PER_SAMPLE:+.1f}cm")
        print(f"  剖面: {' '.join(f'{lag:+d}:{c:+.3f}' for lag, c in zip(lags, cs))}")
        rows.append((side, ild_on, ild_full, ild_max, frac, abs(cs[lags.index(kabs)])))

    # ---- 汇总：真值一致率（H1=正证据判左；H2=镜像）----
    print("=== 汇总（真值 → 各特征判读；判读阈值 ILD ±0.5dB / xcorr ±0.5 采样点）===")
    acc = {"ild_on": [0, 0], "ild_full": [0, 0], "ild_max": [0, 0], "xc": [0, 0]}  # [H1对, H2对]
    print(f"{'轮':<4}{'真值':<4}{'起始ILD':>10}{'判1':>3}{'全窗ILD':>10}{'判1':>3}{'最响ILD':>10}{'判1':>3}{'xc':>7}{'|c|':>6}")
    for i, (side, io, ifu, imx, frac, cabs) in enumerate(rows, 1):
        if io is None:
            print(f"R{i:<3}{side:<4}（无触发/作废）")
            continue

        def vote(v, thr):
            if v is None or abs(v) < thr:
                return "C"
            return "L" if v > 0 else "R"

        v1, v2, v3, v4 = vote(io, 0.5), vote(ifu, 0.5), vote(imx, 0.5), vote(frac, 0.5)
        for key, vv in (("ild_on", v1), ("ild_full", v2), ("ild_max", v3), ("xc", v4)):
            if vv != "C":
                acc[key][0] += vv == side          # H1：正证据=左
                acc[key][1] += vv == {"L": "R", "R": "L", "C": "C"}[side]  # H2：镜像
        print(f"R{i:<3}{side:<4}{io:>+9.2f}dB{v1:>3}{ifu:>+9.2f}dB{v2:>3}{imx:>+9.2f}dB{v3:>3}{frac:>+6.1f} {cabs:>6.3f}")
    print("一致率（H1=正证据判左 / H2=镜像；C 轮判 C 不计分）：")
    for key, name in (("ild_on", "起始ILD"), ("ild_full", "全窗ILD"), ("ild_max", "最响ILD"), ("xc", "xcorr")):
        h1, h2 = acc[key]
        print(f"  {name}: H1={h1} H2={h2}")
    print(f"解析失败 {parse_errors} 次")
    line(fd, "FACE idle", 0.5)


if __name__ == "__main__":
    main()
