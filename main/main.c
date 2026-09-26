/* ESP32 语音实验第四步：AI 智能语音对话（小爱同学风格）
 * 免唤醒：程序一直监听，你直接开口说话，停下来它就自动识别并语音回答
 * 流程：持续监听 → 检测到说话 → 流式上传识别(ASR) → 大模型回答(LLM) → 语音合成(TTS) → 喇叭播放
 * 串口(115200)打印每一轮的识别文字和回答内容
 * 可调参数在下方"配置区"
 */
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <math.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_wifi.h"
#include "esp_event.h"
#include "esp_netif.h"
#include "nvs_flash.h"
#include "esp_http_client.h"
#include "esp_crt_bundle.h"
#include "esp_err.h"
#include "esp_timer.h"
#include "driver/i2s_std.h"
#include "driver/gpio.h"
#include "cJSON.h"

#ifndef M_PI
#define M_PI 3.14159265358979f
#endif

/* ================== 配置区 ================== */
#define WIFI_SSID   "iPhone"
#define WIFI_PASS   "12345678"
#define API_KEY     "sk-tgvtipnqvzonqjxltidszzitsnfraecqjlgpxuoyhkuxsmqj"

#define ASR_MODEL   "Qwen/Qwen3-ASR-1.7B"          /* 语音识别（实测0.4秒出结果，比SenseVoice快100倍） */
#define LLM_MODEL   "Qwen/Qwen2.5-7B-Instruct"     /* 对话大模型；想用 DeepSeek 改 "deepseek-ai/DeepSeek-V3"（收费） */
#define TTS_MODEL   "FunAudioLLM/CosyVoice2-0.5B"  /* 语音合成 */
#define TTS_VOICE   TTS_MODEL ":bella"             /* 女声 bella；男声改 alex */

#define REC_GAIN    1      /* 录音增益：1=放大2倍，0=不放大；识别不准时可试 2(=4倍) */
#define MAX_REC_S   6      /* 一次说话最长秒数（说完自动提前结束+补零，服务端可解析） */
#define VAD_START   3200   /* 确认在说话的阈值（太小环境噪音会误录） */
#define VAD_END     2200   /* 静音阈值：低于它算说完了 */
#define VAD_END_MS  1200   /* 安静多久（毫秒）算说完 */
#define IDLE_RESTART_MS 45000  /* 一直没人说话就重连一次服务器（防连接老化） */
#define PLAY_GAIN   3      /* 播放音量：4=满音量，3=75%，2=50% */
/* ============================================ */

#define API_BASE    "https://api.siliconflow.cn"
#define SAMPLE_RATE 16000
#define CHUNK       256

static i2s_chan_handle_t tx_chan, rx_chan;
static volatile bool got_ip;
static cJSON *g_history;   /* 对话历史（AI 能记住前面聊过什么） */
static int32_t g_dc;       /* 麦克风直流偏移估计 */

/* ---------------- WiFi ---------------- */
static void wifi_event_handler(void *arg, esp_event_base_t base,
                               int32_t id, void *data)
{
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        got_ip = false;
        printf("WiFi 断开，3 秒后重试...\n");
        vTaskDelay(pdMS_TO_TICKS(3000));
        esp_wifi_connect();
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *ev = (ip_event_got_ip_t *)data;
        printf("★ WiFi 已连接！本机 IP: " IPSTR "\n", IP2STR(&ev->ip_info.ip));
        got_ip = true;
    }
}

/* ---------------- I2S（麦克风+功放） ---------------- */
static void i2s_init(void)
{
    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_0, I2S_ROLE_MASTER);
    chan_cfg.dma_frame_num = CHUNK;
    chan_cfg.dma_desc_num = 10;   /* 缓冲 160ms */
    ESP_ERROR_CHECK(i2s_new_channel(&chan_cfg, &tx_chan, &rx_chan));

    i2s_std_config_t std_cfg = {
        .clk_cfg  = I2S_STD_CLK_DEFAULT_CONFIG(SAMPLE_RATE),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_32BIT, I2S_SLOT_MODE_MONO),
        .gpio_cfg = { .mclk = I2S_GPIO_UNUSED, .bclk = GPIO_NUM_4, .ws = GPIO_NUM_5,
                      .dout = GPIO_NUM_25, .din = GPIO_NUM_18,
                      .invert_flags = { 0, 0, 0 } },
    };
    ESP_ERROR_CHECK(i2s_channel_init_std_mode(tx_chan, &std_cfg));
    ESP_ERROR_CHECK(i2s_channel_init_std_mode(rx_chan, &std_cfg));
    ESP_ERROR_CHECK(i2s_channel_enable(tx_chan));
    ESP_ERROR_CHECK(i2s_channel_enable(rx_chan));
}

/* ---------------- 喇叭播放提示音 ---------------- */
static void play_tone(int freq, int ms)
{
    int n = SAMPLE_RATE * ms / 1000;
    int32_t out[CHUNK];
    for (int i = 0; i < n; i += CHUNK) {
        int m = (CHUNK < n - i) ? CHUNK : n - i;
        for (int j = 0; j < m; j++)
            out[j] = (int32_t)(sinf(2 * M_PI * freq * (i + j) / SAMPLE_RATE) * 12000) << 8;
        size_t bw;
        i2s_channel_write(tx_chan, out, m * 4, &bw, 1000);
    }
}

/* ---------------- HTTP 小工具 ---------------- */
static int write_all(esp_http_client_handle_t c, const char *buf, int len)
{
    int off = 0;
    while (off < len) {
        int w = esp_http_client_write(c, buf + off, len - off);
        if (w <= 0) return -1;
        off += w;
    }
    return 0;
}

/* 读完响应体，返回 HTTP 状态码 */
static int read_body(esp_http_client_handle_t c, char *resp, int resp_sz)
{
    esp_http_client_fetch_headers(c);
    int total = 0;
    while (total < resp_sz - 1) {
        int r = esp_http_client_read(c, resp + total, resp_sz - 1 - total);
        if (r <= 0) break;
        total += r;
    }
    resp[total] = 0;
    return esp_http_client_get_status_code(c);
}

static void set_auth(esp_http_client_handle_t c)
{
    char auth[96];
    snprintf(auth, sizeof(auth), "Bearer %s", API_KEY);
    esp_http_client_set_header(c, "Authorization", auth);
}

/* ---------------- WAV 头（44 字节） ---------------- */
static void put32(uint8_t *p, uint32_t v) { p[0]=v; p[1]=v>>8; p[2]=v>>16; p[3]=v>>24; }
static void put16(uint8_t *p, uint16_t v) { p[0]=v; p[1]=v>>8; }

static void build_wav_header(uint8_t *b, int data_len)
{
    memcpy(b, "RIFF", 4);
    put32(b + 4, 36 + data_len);
    memcpy(b + 8, "WAVEfmt ", 8);
    put32(b + 16, 16);            /* fmt 块大小 */
    put16(b + 20, 1);             /* PCM */
    put16(b + 22, 1);             /* 单声道 */
    put32(b + 24, SAMPLE_RATE);
    put32(b + 28, SAMPLE_RATE * 2);  /* 字节率 */
    put16(b + 32, 2);             /* 块对齐 */
    put16(b + 34, 16);            /* 位深 */
    memcpy(b + 36, "data", 4);
    put32(b + 40, data_len);
}

/* ---------------- [2] 语音识别：固定长度流式上传 ----------------
 * 服务端不支持 chunked 分块上传（实测 500），改用固定 Content-Length：
 * 先声明 MAX_REC_S 秒总长，说完提前结束，剩余用静音(零)补齐——已验证可正常识别 */
#define ASR_BND "ESP32VOICE"
#define ASR_TAIL "\r\n--" ASR_BND "--\r\n"

typedef struct {
    esp_http_client_handle_t c;
    char resp[1024];
    int sent;                  /* 已发送的 WAV 数据字节数 */
} asr_stream_t;

/* 开始：连接识别服务并写好 multipart 头 + WAV 头（握手约 1~2 秒） */
static int asr_begin(asr_stream_t *s)
{
    esp_http_client_config_t cfg = {
        .url = API_BASE "/v1/audio/transcriptions", .method = HTTP_METHOD_POST,
        .timeout_ms = 90000, .crt_bundle_attach = esp_crt_bundle_attach,
        .buffer_size = 2048, .buffer_size_tx = 4096,
    };
    s->c = esp_http_client_init(&cfg);
    if (!s->c) return -1;
    set_auth(s->c);
    char ct[64], head[384];
    snprintf(ct, sizeof(ct), "multipart/form-data; boundary=%s", ASR_BND);
    esp_http_client_set_header(s->c, "Content-Type", ct);
    int n1 = snprintf(head, sizeof(head),
        "--" ASR_BND "\r\n"
        "Content-Disposition: form-data; name=\"model\"\r\n\r\n"
        ASR_MODEL "\r\n"
        "--" ASR_BND "\r\n"
        "Content-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\n"
        "Content-Type: audio/wav\r\n\r\n");
    uint8_t wav[44];
    build_wav_header(wav, SAMPLE_RATE * MAX_REC_S * 2);  /* WAV 头声明最长秒数 */
    int total = n1 + 44 + SAMPLE_RATE * MAX_REC_S * 2 + (int)strlen(ASR_TAIL);

    if (esp_http_client_open(s->c, total) != ESP_OK ||
        write_all(s->c, head, n1) < 0 ||
        write_all(s->c, (const char *)wav, 44) < 0) {
        esp_http_client_cleanup(s->c);
        s->c = NULL;
        return -1;
    }
    s->sent = 0;
    return 0;
}

/* 上传一段音频数据（len 为字节数） */
static int asr_chunk(asr_stream_t *s, const void *data, int len)
{
    if (len <= 0) return 0;
    if (write_all(s->c, (const char *)data, len) < 0) return -1;
    s->sent += len;
    return 0;
}

/* 结束：不足总长的部分补零，然后读取识别结果 */
static int asr_finish(asr_stream_t *s, char *text, int text_sz)
{
    int err = -1;
    int remain = SAMPLE_RATE * MAX_REC_S * 2 - s->sent;
    char zbuf[2048];
    memset(zbuf, 0, sizeof(zbuf));
    int ok = 1;
    while (remain > 0 && ok) {           /* 补零到声明的总长度 */
        int m = (remain < (int)sizeof(zbuf)) ? remain : (int)sizeof(zbuf);
        if (write_all(s->c, zbuf, m) < 0) ok = 0;
        remain -= m;
    }
    if (ok && write_all(s->c, ASR_TAIL, strlen(ASR_TAIL)) == 0) {
        int status = read_body(s->c, s->resp, sizeof(s->resp));
        if (status == 200) {
            cJSON *j = cJSON_Parse(s->resp);
            if (j) {
                char *t = cJSON_GetStringValue(cJSON_GetObjectItem(j, "text"));
                if (t) { snprintf(text, text_sz, "%s", t); err = 0; }
                cJSON_Delete(j);
            }
        } else {
            printf("识别接口 HTTP %d: %s\n", status, s->resp);
        }
    }
    esp_http_client_cleanup(s->c);
    s->c = NULL;
    return err;
}

/* ---------------- JSON POST 通用函数 ---------------- */
static int api_post_json(const char *url, const char *body, int body_len,
                         char *resp, int resp_sz)
{
    esp_http_client_config_t cfg = {
        .url = url, .method = HTTP_METHOD_POST, .timeout_ms = 60000,
        .crt_bundle_attach = esp_crt_bundle_attach,
        .buffer_size = 4096, .buffer_size_tx = 4096,
    };
    esp_http_client_handle_t c = esp_http_client_init(&cfg);
    if (!c) return -1;
    set_auth(c);
    esp_http_client_set_header(c, "Content-Type", "application/json");
    int status = -1;
    if (esp_http_client_open(c, body_len) == ESP_OK && write_all(c, body, body_len) == 0)
        status = read_body(c, resp, resp_sz);
    esp_http_client_cleanup(c);
    return status;
}

/* ---------------- [3] 大模型对话（带历史记忆） ---------------- */
static int do_llm(const char *user_text, char *answer, int ans_sz)
{
    cJSON *m = cJSON_CreateObject();
    cJSON_AddStringToObject(m, "role", "user");
    cJSON_AddStringToObject(m, "content", user_text);
    cJSON_AddItemToArray(g_history, m);
    while (cJSON_GetArraySize(g_history) > 9) {   /* 最多记 4 轮对话 */
        cJSON_DeleteItemFromArray(g_history, 1);
        cJSON_DeleteItemFromArray(g_history, 1);
    }

    cJSON *body = cJSON_CreateObject();
    cJSON_AddStringToObject(body, "model", LLM_MODEL);
    cJSON_AddItemToObject(body, "messages", cJSON_Duplicate(g_history, 1));
    cJSON_AddNumberToObject(body, "max_tokens", 150);
    char *post = cJSON_PrintUnformatted(body);
    cJSON_Delete(body);

    char *resp = malloc(8192);
    int err = -1;
    int status = api_post_json(API_BASE "/v1/chat/completions",
                               post, strlen(post), resp, 8192);
    if (status == 200) {
        cJSON *j = cJSON_Parse(resp);
        if (j) {
            cJSON *ch = cJSON_GetArrayItem(cJSON_GetObjectItem(j, "choices"), 0);
            char *t = cJSON_GetStringValue(
                cJSON_GetObjectItem(cJSON_GetObjectItem(ch, "message"), "content"));
            if (t) {
                snprintf(answer, ans_sz, "%s", t);
                cJSON *a = cJSON_CreateObject();
                cJSON_AddStringToObject(a, "role", "assistant");
                cJSON_AddStringToObject(a, "content", answer);
                cJSON_AddItemToArray(g_history, a);
                err = 0;
            }
            cJSON_Delete(j);
        }
    } else {
        printf("大模型接口 HTTP %d: %s\n", status, resp);
    }
    free(post);
    free(resp);
    return err;
}

/* ---------------- [4] 语音合成 + 流式播放 ---------------- */
static int do_tts_play(const char *text)
{
    cJSON *body = cJSON_CreateObject();
    cJSON_AddStringToObject(body, "model", TTS_MODEL);
    cJSON_AddStringToObject(body, "input", text);
    cJSON_AddStringToObject(body, "voice", TTS_VOICE);
    cJSON_AddStringToObject(body, "response_format", "pcm");
    cJSON_AddNumberToObject(body, "sample_rate", SAMPLE_RATE);
    char *post = cJSON_PrintUnformatted(body);
    cJSON_Delete(body);

    esp_http_client_config_t cfg = {
        .url = API_BASE "/v1/audio/speech", .method = HTTP_METHOD_POST,
        .timeout_ms = 60000, .crt_bundle_attach = esp_crt_bundle_attach,
        .buffer_size = 4096, .buffer_size_tx = 4096,
    };
    esp_http_client_handle_t c = esp_http_client_init(&cfg);
    if (!c) { free(post); return -1; }
    set_auth(c);
    esp_http_client_set_header(c, "Content-Type", "application/json");

    int plen = strlen(post), err = -1;
    if (esp_http_client_open(c, plen) == ESP_OK && write_all(c, post, plen) == 0) {
        esp_http_client_fetch_headers(c);
        int status = esp_http_client_get_status_code(c);
        if (status == 200) {
            int16_t *in = malloc(1024);    /* 512 个采样 */
            int32_t *out = malloc(2048);
            if (in && out) {
                err = 0;
                int r;
                while ((r = esp_http_client_read(c, (char *)in, 1024)) > 0) {
                    int n = r / 2;
                    for (int i = 0; i < n; i++)
                        out[i] = (((int32_t)in[i] * PLAY_GAIN) / 4) << 16;
                    size_t bw;
                    if (i2s_channel_write(tx_chan, out, n * 4, &bw, 2000) != ESP_OK) {
                        err = -1;
                        break;
                    }
                }
            }
            free(in);
            free(out);
            int32_t zero[64] = {0};        /* 150ms 静音收尾防截断 */
            for (int i = 0; i < 6; i++) {
                size_t bw;
                i2s_channel_write(tx_chan, zero, sizeof(zero), &bw, 100);
            }
        } else {
            char eb[512] = {0};
            int t2 = 0, r;
            while (t2 < 511 && (r = esp_http_client_read(c, eb + t2, 511 - t2)) > 0)
                t2 += r;
            printf("合成接口 HTTP %d: %s\n", status, eb);
        }
    }
    esp_http_client_cleanup(c);
    free(post);
    return err;
}

/* ---------------- 麦克风采样处理：去直流 + 增益，返回峰值 ---------------- */
static int mic_process(int32_t raw, int16_t *out)
{
    int32_t x = raw >> 16;
    g_dc += (x - g_dc) >> 8;          /* 慢速跟踪直流偏移（约10Hz高通） */
    int32_t y = x - g_dc;
    int peak = (y < 0) ? -y : y;      /* 峰值用原始幅度（VAD 判断用） */
    int32_t g = y << REC_GAIN;        /* 上传的音频加增益 */
    if (g > 32000) g = 32000;
    else if (g < -32000) g = -32000;
    *out = (int16_t)g;
    return peak;
}

/* ---------------- 主对话任务 ---------------- */
static void chat_task(void *arg)
{
    /* 等联网（最多 30 秒） */
    int w = 0;
    while (!got_ip && w++ < 150)
        vTaskDelay(pdMS_TO_TICKS(200));
    if (!got_ip) {
        printf("✘ WiFi 没连上，请重启板子重试\n");
        vTaskDelete(NULL);
    }
    printf("★ 网络就绪，进入语音对话模式\n");

    play_tone(880, 150);
    play_tone(1320, 250);   /* 就绪提示音 */

    /* 系统提示词 */
    g_history = cJSON_CreateArray();
    cJSON *sys = cJSON_CreateObject();
    cJSON_AddStringToObject(sys, "role", "system");
    cJSON_AddStringToObject(sys, "content",
        "你是一个中文语音助手，回答要口语化、简短，一般不超过50个字，"
        "不要用列表、符号和表情。");
    cJSON_AddItemToArray(g_history, sys);

    int32_t raw[CHUNK];
    int16_t tmp[CHUNK];
    int round = 0;
    while (1) {
        round++;
        printf("\n===== 第 %d 轮 | 请说话 =====\n", round);

        /* 先连接识别服务再等你说话：握手期间的安静不会丢任何声音 */
        printf("（连接识别服务...）\n");
        fflush(stdout);
        asr_stream_t asr = { .c = NULL };
        if (asr_begin(&asr) != 0) {
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
            if (i2s_channel_read(rx_chan, raw, sizeof(raw), &br, 1000) != ESP_OK) {
                vTaskDelay(pdMS_TO_TICKS(10));
                continue;
            }
            int n = br / 4, wp = 0;
            for (int i = 0; i < n; i++) {
                int p = mic_process(raw[i], &tmp[i]);
                if (p > wp) wp = p;
            }
            int ms = n * 1000 / SAMPLE_RATE;
            since_ms += ms;
            if (asr_chunk(&asr, tmp, n * 2) < 0) {
                if (!loud) {   /* 还没说话连接就断了：重连一次继续听 */
                    printf("（连接中断，重连...）\n");
                    esp_http_client_cleanup(asr.c);
                    if (asr_begin(&asr) != 0) { up_err = true; break; }
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
                    esp_http_client_cleanup(asr.c);
                    if (asr_begin(&asr) != 0) { up_err = true; break; }
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
                /* 已发字节达到声明长度（预缓冲+录音），强制结束防止发超 */
                if (asr.sent + n * 2 > SAMPLE_RATE * MAX_REC_S * 2) break;
            }
        }

        /* ---- 识别结果 ---- */
        char *text = malloc(512);
        int aerr = 0;
        int64_t t0 = esp_timer_get_time();
        if (up_err) {
            esp_http_client_cleanup(asr.c);
        } else {
            printf("[语音识别中...]（服务器忙时可能要等几十秒）\n");
            aerr = asr_finish(&asr, text, 512);
        }
        if (up_err || aerr != 0 || text[0] == 0) {
            printf("（没听清，3 秒后再试）\n");
            free(text);
            play_tone(400, 120);
            vTaskDelay(pdMS_TO_TICKS(3000));
            continue;
        }
        printf("    你说: %s（识别耗时 %d 秒）\n", text,
               (int)((esp_timer_get_time() - t0) / 1000000));

        /* ---- 大模型回答 ---- */
        char *answer = malloc(1024);
        printf("[AI 思考中...]\n");
        if (do_llm(text, answer, 1024) != 0 || answer[0] == 0) {
            printf("✘ AI 回答失败\n");
            free(text); free(answer);
            play_tone(400, 150);
            continue;
        }
        printf("    AI说: %s\n", answer);
        free(text);

        /* ---- 合成 + 播放 ---- */
        printf("[播放回答...]\n");
        if (do_tts_play(answer) != 0)
            printf("✘ 播放失败\n");
        free(answer);

        /* 播完把喇叭回声从麦克风里排干，防止下一轮把回声当说话 */
        for (int i = 0; i < 75; i++) {
            size_t br;
            i2s_channel_read(rx_chan, raw, sizeof(raw), &br, 100);
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
    esp_netif_init();
    esp_event_loop_create_default();
    esp_netif_create_default_wifi_sta();
    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&cfg));
    esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, &wifi_event_handler, NULL);
    esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, &wifi_event_handler, NULL);
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    wifi_config_t wc = { .sta = { .ssid = WIFI_SSID, .password = WIFI_PASS } };
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &wc));
    ESP_ERROR_CHECK(esp_wifi_start());
    esp_wifi_set_ps(WIFI_PS_NONE);   /* 关 WiFi 省电模式，降低网络延迟 */
    esp_wifi_connect();

    printf("\n=== AI 语音助手（免唤醒，直接说话） ===\n");
    i2s_init();
    xTaskCreate(chat_task, "chat", 32768, NULL, 5, NULL);
}
