/* RockRobot3（马克3号）—— AI 语音管家
 * 免唤醒：程序一直监听，你直接开口说话，停下来它就自动识别并语音回答
 * 链路：麦克风录音 → 云端语音识别(asr) → 大模型回答(llm) → 语音合成(tts) → 喇叭播放
 * 附加：说"开灯/关灯"直接控制板载继电器（本地关键词，无需等 AI）
 * 日志全量进 core/log（面板日志页/蓝牙 logs 命令可回放）；运行数据进 core/data
 * 可调参数统一在 config.h（对话策略）与各模块的 *_config.h
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "nvs_flash.h"
#include "esp_system.h"
#include "esp_timer.h"
#include "esp_err.h"
#include "esp_app_desc.h"
#include "esp_task_wdt.h"
#include "config.h"
#include "wifi.h"
#include "audio.h"
#include "relay.h"
#include "panel.h"
#include "asr.h"
#include "llm.h"
#include "tts.h"
#include "applog.h"
#include "data.h"
#include "btctl.h"
#include "nearlink.h"
#include "eth.h"

/* ---------------- TTS 回调：PCM 增益放大后送喇叭 ---------------- */
static int32_t s_pcm32[1024];
static esp_err_t tts_sink(const int16_t *pcm, int samples, void *ctx)
{
    (void)ctx;
    for (int i = 0; i < samples; i++)
        s_pcm32[i] = (((int32_t)pcm[i] * PLAY_GAIN) / 4) << 16;
    return audio_write(s_pcm32, samples * 4, 2000);
}

/* ---------------- 主对话任务 ---------------- */
static void chat_task(void *arg)
{
    /* 注册本任务到看门狗：必须在任何 esp_task_wdt_reset() 之前，
     * 否则"喂狗给未注册任务"会每 200ms 刷一条 task not found 错误 */
    esp_task_wdt_add(NULL);

    /* 等联网：一直等（配网/断线重连可能要几十秒到几分钟），每 10 秒提示一次。
     * 不能等超时就退出——那会让语音助手在首次配网慢的机器上永久失效 */
    int w = 0;
    while (!wifi_connected()) {
        if (w % 50 == 0)
            log_writef("（等待网络连接...）");
        w++;
        esp_task_wdt_reset();
        vTaskDelay(pdMS_TO_TICKS(200));
    }
    log_writef("★ 网络就绪，进入语音对话模式");

    audio_play_tone(880, 150);
    audio_play_tone(1320, 250);   /* 就绪提示音 */

    llm_chat_init();

    /* 单轮 HTTP 最长 90 秒，看门狗超时阈值在 sdkconfig 调大 */

    int32_t raw[AUDIO_CHUNK];
    int16_t tmp[AUDIO_CHUNK];
    int round = 0;
    while (1) {
        esp_task_wdt_reset();
        round++;

        /* 麦克风被静音（面板/蓝牙关的）：完全不听不连云端 */
        if (!audio_mic_enabled()) {
            vTaskDelay(pdMS_TO_TICKS(200));
            continue;
        }

        log_writef("===== 第 %d 轮 | 连接识别服务 =====", round);

        /* 先连接识别服务再等你说话：握手期间的安静不会丢任何声音 */
        asr_t *asr = asr_begin(AUDIO_SAMPLE_RATE, MAX_REC_S);
        if (!asr) {
            data_inc(DATA_ERRORS);
            log_writef("✘ 连不上识别服务，3 秒后重试");
            vTaskDelay(pdMS_TO_TICKS(3000));
            continue;
        }
        log_writef("（就绪，请说话）");

        /* 环形预缓冲：VAD 触发前 0.75 秒的声音（防丢开头），静音期间不上传（防服务端背压卡死） */
        static int16_t prebuf[AUDIO_SAMPLE_RATE * 3 / 4];
        int pre_len = 0;
        int loud_f = 0, silent_ms = 0, since_ms = 0;
        bool loud = false, up_err = false;

        /* ---- 监听 + 自动断句（说完自动提交） ---- */
        while (1) {
            size_t br;
            if (audio_read(raw, sizeof(raw), &br, 1000) != ESP_OK || br == 0) {
                vTaskDelay(pdMS_TO_TICKS(10));
                if (!audio_mic_enabled()) { up_err = true; break; }  /* 录音中途被静音 */
                continue;
            }
            int n = br / 4, wp = 0;
            for (int i = 0; i < n; i++) {
                int p = audio_mic_process(raw[i], &tmp[i]);
                if (p > wp) wp = p;
            }
            int ms = n * 1000 / AUDIO_SAMPLE_RATE;
            since_ms += ms;
            /* 静音期间只进预缓冲不上传：每 chunk 先缓存，触发时一次补传 */
            if (pre_len + n > AUDIO_SAMPLE_RATE * 3 / 4) {
                int drop = pre_len + n - AUDIO_SAMPLE_RATE * 3 / 4;
                memmove(prebuf, prebuf + drop, (pre_len - drop) * 2);
                pre_len -= drop;
            }
            memcpy(prebuf + pre_len, tmp, n * 2);
            pre_len += n;
            if (!loud) {
                if (wp >= VAD_START) {
                    if (++loud_f >= 2) {
                        loud = true;
                        log_writef("（录音中，说完自动停）");
                        /* 补传触发前的声音（预缓冲），此后说话内容实时上传 */
                        if (asr_chunk(asr, prebuf, pre_len * 2) < 0) { up_err = true; break; }
                        pre_len = 0;
                    }
                } else {
                    loud_f = 0;
                }
                if (!loud && since_ms > IDLE_RESTART_MS) {
                    /* 一直没人说话：重连一次，防连接老化 */
                    asr_abort(asr);
                    if ((asr = asr_begin(AUDIO_SAMPLE_RATE, MAX_REC_S)) == NULL) { up_err = true; break; }
                    log_writef("（就绪，请说话）");
                    since_ms = 0;
                    pre_len = 0;
                }
            } else {
                if (asr_chunk(asr, tmp, n * 2) < 0) { up_err = true; break; }   /* 上传失败直接结束本轮 */
                if (wp < VAD_END) {
                    silent_ms += ms;
                    if (silent_ms >= VAD_END_MS) break;   /* 说完了 */
                } else {
                    silent_ms = 0;
                }
                /* 已发字节达到声明长度，强制结束防止发超 */
                if (asr_sent(asr) + n * 2 > AUDIO_SAMPLE_RATE * MAX_REC_S * 2) break;
            }
        }

        /* ---- 识别结果 ---- */
        char *text = malloc(512);
        int aerr = 0;
        int64_t t0 = esp_timer_get_time();
        if (up_err) {
            asr_abort(asr);
        } else {
            log_writef("[语音识别中...]");
            aerr = asr_finish(asr, text, 512);
        }
        if (up_err || aerr != 0 || text[0] == 0) {
            data_inc(DATA_ASR_FAIL);
            log_writef("（没听清，3 秒后再试）");
            free(text);
            audio_play_tone(400, 120);
            vTaskDelay(pdMS_TO_TICKS(3000));
            continue;
        }
        data_inc(DATA_ASR_OK);
        log_writef("    你说: %s（识别耗时 %d 秒）", text,
                   (int)((esp_timer_get_time() - t0) / 1000000));

        /* ---- 本地语音控制：继电器开关灯（不用等 AI，立刻响应） ---- */
        if (strstr(text, "开灯") || strstr(text, "打开灯") || strstr(text, "把灯打开")) {
            relay_set(true);
            data_inc(DATA_RELAY_ON);
            log_writef("    >> 继电器吸合（灯开）");
        } else if (strstr(text, "关灯") || strstr(text, "关闭灯") || strstr(text, "把灯关")) {
            relay_set(false);
            data_inc(DATA_RELAY_OFF);
            log_writef("    >> 继电器断开（灯关）");
        }

        /* ---- 大模型回答 ---- */
        char *answer = malloc(1024);
        log_writef("[AI 思考中...]");
        if (llm_chat(text, answer, 1024) != 0 || answer[0] == 0) {
            data_inc(DATA_LLM_FAIL);
            log_writef("✘ AI 回答失败");
            free(text); free(answer);
            audio_play_tone(400, 150);
            continue;
        }
        log_writef("    AI说: %s", answer);
        free(text);

        /* ---- 合成 + 播放（tts 模块回调交付 PCM，这里送喇叭） ---- */
        log_writef("[播放回答...]");
        if (tts_play(answer, AUDIO_SAMPLE_RATE, tts_sink, NULL) != 0) {
            data_inc(DATA_TTS_FAIL);
            log_writef("✘ 播放失败");
        }
        int32_t zero[64] = {0};        /* 150ms 静音收尾防截断 */
        for (int i = 0; i < 6; i++)
            audio_write(zero, sizeof(zero), 100);
        free(answer);

        data_inc(DATA_CHAT_ROUNDS);

        /* 播完把喇叭回声从麦克风里排干，防止下一轮把回声当说话 */
        for (int i = 0; i < 75; i++) {
            size_t br;
            audio_read(raw, sizeof(raw), &br, 100);
        }
        vTaskDelay(pdMS_TO_TICKS(500));
    }
}

void app_main(void)
{
    esp_err_t ret = nvs_flash_init();
    if (ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ESP_ERROR_CHECK(nvs_flash_init());
    }

    log_init();      /* 尽早接管日志：之后所有 ESP_LOGx 都进 RAM 缓冲可回放 */
    data_init();     /* 运行数据载入 NVS，开机次数 +1 */

    /* 看门狗超时改 120 秒：对话任务单轮云端 HTTP 最长 90 秒是设计行为。
     * 注意 sdkconfig 的 ESP_TASK_WDT_TIMEOUT_S 只允许 1~60（120 会被静默丢弃回退 5s），
     * 所以这里用运行时 API 重配 */
    const esp_task_wdt_config_t twdt_cfg = {
        .timeout_ms = 120000,
        .idle_core_mask = (1 << 0) | (1 << 1),   /* 保持监控两个 IDLE 任务 */
        .trigger_panic = false,
    };
    ESP_ERROR_CHECK(esp_task_wdt_reconfigure(&twdt_cfg));

    const esp_app_desc_t *app = esp_app_get_description();
    log_writef("=== RockRobot3（马克3号）v%s | 免唤醒语音助手 ===", app->version);

    wifi_start();
    panel_start();
    audio_init();
    relay_init();
    /* 对话任务必须在 bt_start 之前创建：Bluedroid 初始化要吃掉 ~50KB 堆，
     * 之后连 16KB 的任务栈都分配不出来（实测 16384/32768 均失败） */
    if (xTaskCreate(chat_task, "chat", 32768, NULL, 5, NULL) != pdPASS)
        log_writef("✘ 对话任务创建失败（内存不足），请重启设备");
    else
        log_writef("对话任务已启动（空闲堆 %u KB）",
                   (unsigned)(esp_get_free_heap_size() / 1024));
    bt_start();      /* 经典蓝牙 SPP 控制台（不依赖 WiFi） */
    nearlink_start();/* 星闪/以太网：接口预留，日志提示硬件状态 */
    eth_start();
}
