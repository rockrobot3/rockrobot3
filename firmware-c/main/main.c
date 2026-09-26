/* AI 语音管家（小爱同学风格）
 * 免唤醒：程序一直监听，你直接开口说话，停下来它就自动识别并语音回答
 * 链路：麦克风录音 → 云端语音识别(ASR) → 大模型回答(LLM) → 语音合成(TTS) → 喇叭播放
 * 附加：说"开灯/关灯"直接控制板载继电器（本地关键词，无需等 AI）
 * 串口(115200)打印每一轮的识别文字和回答内容
 * 可调参数统一在 config.h
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "nvs_flash.h"
#include "esp_timer.h"
#include "esp_err.h"
#include "config.h"
#include "app_wifi.h"
#include "app_audio.h"
#include "app_cloud.h"

/* ---------------- 主对话任务 ---------------- */
static void chat_task(void *arg)
{
    /* 等联网（最多 30 秒） */
    int w = 0;
    while (!app_wifi_connected() && w++ < 150)
        vTaskDelay(pdMS_TO_TICKS(200));
    if (!app_wifi_connected()) {
        printf("✘ WiFi 没连上，请重启板子重试\n");
        vTaskDelete(NULL);
    }
    printf("★ 网络就绪，进入语音对话模式\n");

    audio_play_tone(880, 150);
    audio_play_tone(1320, 250);   /* 就绪提示音 */

    cloud_history_init();

    int32_t raw[CHUNK];
    int16_t tmp[CHUNK];
    int round = 0;
    while (1) {
        round++;
        printf("\n===== 第 %d 轮 | 请说话 =====\n", round);

        /* 先连接识别服务再等你说话：握手期间的安静不会丢任何声音 */
        printf("（连接识别服务...）\n");
        fflush(stdout);
        cloud_asr_t asr = { .c = NULL };
        if (cloud_asr_begin(&asr) != 0) {
            printf("✘ 连不上识别服务，3 秒后重试\n");
            vTaskDelay(pdMS_TO_TICKS(3000));
            continue;
        }
        printf("（就绪，请说话）\n");

        int loud_f = 0, silent_ms = 0, since_ms = 0;
        bool loud = false, up_err = false;

        /* ---- 监听 + 自动断句（说完自动提交） ---- */
        while (1) {
            size_t br;
            if (audio_read(raw, sizeof(raw), &br, 1000) != ESP_OK) {
                vTaskDelay(pdMS_TO_TICKS(10));
                continue;
            }
            int n = br / 4, wp = 0;
            for (int i = 0; i < n; i++) {
                int p = audio_mic_process(raw[i], &tmp[i]);
                if (p > wp) wp = p;
            }
            int ms = n * 1000 / SAMPLE_RATE;
            since_ms += ms;
            if (cloud_asr_chunk(&asr, tmp, n * 2) < 0) {
                if (!loud) {   /* 还没说话连接就断了：重连一次继续听 */
                    printf("（连接中断，重连...）\n");
                    cloud_asr_abort(&asr);
                    if (cloud_asr_begin(&asr) != 0) { up_err = true; break; }
                    since_ms = 0;
                    continue;
                }
                up_err = true;
                break;
            }
            if (!loud) {
                if (wp >= VAD_START) {
                    if (++loud_f >= 2) {
                        loud = true;
                        printf("（录音中，说完自动停）\n");
                    }
                } else {
                    loud_f = 0;
                }
                if (!loud && since_ms > IDLE_RESTART_MS) {
                    /* 一直没人说话：重连一次，防连接老化 */
                    cloud_asr_abort(&asr);
                    if (cloud_asr_begin(&asr) != 0) { up_err = true; break; }
                    printf("（就绪，请说话）\n");
                    since_ms = 0;
                }
            } else {
                if (wp < VAD_END) {
                    silent_ms += ms;
                    if (silent_ms >= VAD_END_MS) break;   /* 说完了 */
                } else {
                    silent_ms = 0;
                }
                /* 已发字节达到声明长度，强制结束防止发超 */
                if (asr.sent + n * 2 > SAMPLE_RATE * MAX_REC_S * 2) break;
            }
        }

        /* ---- 识别结果 ---- */
        char *text = malloc(512);
        int aerr = 0;
        int64_t t0 = esp_timer_get_time();
        if (up_err) {
            cloud_asr_abort(&asr);
        } else {
            printf("[语音识别中...]\n");
            aerr = cloud_asr_finish(&asr, text, 512);
        }
        if (up_err || aerr != 0 || text[0] == 0) {
            printf("（没听清，3 秒后再试）\n");
            free(text);
            audio_play_tone(400, 120);
            vTaskDelay(pdMS_TO_TICKS(3000));
            continue;
        }
        printf("    你说: %s（识别耗时 %d 秒）\n", text,
               (int)((esp_timer_get_time() - t0) / 1000000));

        /* ---- 本地语音控制：继电器开关灯（不用等 AI，立刻响应） ---- */
        if (strstr(text, "开灯") || strstr(text, "打开灯") || strstr(text, "把灯打开")) {
            relay_set(true);
            printf("    >> 继电器吸合（灯开）\n");
        } else if (strstr(text, "关灯") || strstr(text, "关闭灯") || strstr(text, "把灯关")) {
            relay_set(false);
            printf("    >> 继电器断开（灯关）\n");
        }

        /* ---- 大模型回答 ---- */
        char *answer = malloc(1024);
        printf("[AI 思考中...]\n");
        if (cloud_llm_chat(text, answer, 1024) != 0 || answer[0] == 0) {
            printf("✘ AI 回答失败\n");
            free(text); free(answer);
            audio_play_tone(400, 150);
            continue;
        }
        printf("    AI说: %s\n", answer);
        free(text);

        /* ---- 合成 + 播放 ---- */
        printf("[播放回答...]\n");
        if (cloud_tts_play(answer) != 0)
            printf("✘ 播放失败\n");
        free(answer);

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

    printf("\n=== AI 语音管家（免唤醒，直接说话） ===\n");
    app_wifi_start();
    audio_init();
    relay_init();
    xTaskCreate(chat_task, "chat", 32768, NULL, 5, NULL);
}
