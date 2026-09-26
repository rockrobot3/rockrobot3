/* llm 模块实现：流式 ASR + LLM 对话(带记忆) + TTS(PCM 回调输出)
 * 从 firmware-c 的 app_cloud.c 迁移而来，去掉了对固件音频模块的依赖 */
#include "llm.h"
#include "llm_config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "esp_http_client.h"
#include "esp_crt_bundle.h"
#include "cJSON.h"

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

static void build_wav_header(uint8_t *b, int sample_rate, int data_len)
{
    memcpy(b, "RIFF", 4);
    put32(b + 4, 36 + data_len);
    memcpy(b + 8, "WAVEfmt ", 8);
    put32(b + 16, 16);                  /* fmt 块大小 */
    put16(b + 20, 1);                   /* PCM */
    put16(b + 22, 1);                   /* 单声道 */
    put32(b + 24, sample_rate);
    put32(b + 28, sample_rate * 2);     /* 字节率 */
    put16(b + 32, 2);                   /* 块对齐 */
    put16(b + 34, 16);                  /* 位深 */
    memcpy(b + 36, "data", 4);
    put32(b + 40, data_len);
}

/* ---------------- 流式语音识别：固定长度上传 ----------------
 * 服务端不支持 chunked 分块上传（实测 500），改用固定 Content-Length：
 * 先声明 max_seconds 秒总长，说完提前结束，剩余用静音(零)补齐——已验证可正常识别 */
#define ASR_BND "ESP32VOICE"
#define ASR_TAIL "\r\n--" ASR_BND "--\r\n"

struct llm_asr_s {
    esp_http_client_handle_t c;
    char resp[1024];
    int sent;        /* 已发送的 PCM 字节数 */
    int max_bytes;   /* 声明的 PCM 总字节数 */
};

llm_asr_t *llm_asr_begin(int sample_rate, int max_seconds)
{
    llm_asr_t *s = calloc(1, sizeof(llm_asr_t));
    if (!s) return NULL;
    s->max_bytes = sample_rate * max_seconds * 2;

    esp_http_client_config_t cfg = {
        .url = API_BASE "/v1/audio/transcriptions", .method = HTTP_METHOD_POST,
        .timeout_ms = ASR_TIMEOUT_MS, .crt_bundle_attach = esp_crt_bundle_attach,
        .buffer_size = 2048, .buffer_size_tx = 4096,
    };
    s->c = esp_http_client_init(&cfg);
    if (!s->c) { free(s); return NULL; }
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
    build_wav_header(wav, sample_rate, s->max_bytes);  /* WAV 头声明最长秒数 */
    int total = n1 + 44 + s->max_bytes + (int)strlen(ASR_TAIL);

    if (esp_http_client_open(s->c, total) != ESP_OK ||
        write_all(s->c, head, n1) < 0 ||
        write_all(s->c, (const char *)wav, 44) < 0) {
        esp_http_client_cleanup(s->c);
        free(s);
        return NULL;
    }
    return s;
}

int llm_asr_chunk(llm_asr_t *s, const void *data, int len)
{
    if (!s || len <= 0) return 0;
    if (write_all(s->c, (const char *)data, len) < 0) return -1;
    s->sent += len;
    return 0;
}

int llm_asr_sent(llm_asr_t *s)
{
    return s ? s->sent : 0;
}

void llm_asr_abort(llm_asr_t *s)
{
    if (!s) return;
    if (s->c) {
        esp_http_client_cleanup(s->c);
        s->c = NULL;
    }
    free(s);
}

int llm_asr_finish(llm_asr_t *s, char *text, int text_sz)
{
    int err = -1;
    int remain = s->max_bytes - s->sent;
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
    free(s);
    return err;
}

/* ---------------- JSON POST 通用函数 ---------------- */
static int api_post_json(const char *url, const char *body, int body_len,
                         char *resp, int resp_sz)
{
    esp_http_client_config_t cfg = {
        .url = url, .method = HTTP_METHOD_POST, .timeout_ms = LLM_TIMEOUT_MS,
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

/* ---------------- 大模型对话（带历史记忆） ---------------- */
static cJSON *s_history;

int llm_chat_init(void)
{
    s_history = cJSON_CreateArray();
    cJSON *sys = cJSON_CreateObject();
    cJSON_AddStringToObject(sys, "role", "system");
    cJSON_AddStringToObject(sys, "content", SYSTEM_PROMPT);
    cJSON_AddItemToArray(s_history, sys);
    return 0;
}

int llm_chat(const char *user_text, char *answer, int ans_sz)
{
    if (!s_history) llm_chat_init();

    cJSON *m = cJSON_CreateObject();
    cJSON_AddStringToObject(m, "role", "user");
    cJSON_AddStringToObject(m, "content", user_text);
    cJSON_AddItemToArray(s_history, m);
    while (cJSON_GetArraySize(s_history) > 1 + LLM_HISTORY_ROUNDS * 2) {
        cJSON_DeleteItemFromArray(s_history, 1);
        cJSON_DeleteItemFromArray(s_history, 1);
    }

    cJSON *body = cJSON_CreateObject();
    cJSON_AddStringToObject(body, "model", LLM_MODEL);
    cJSON_AddItemToObject(body, "messages", cJSON_Duplicate(s_history, 1));
    cJSON_AddNumberToObject(body, "max_tokens", LLM_MAX_TOKENS);
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
                cJSON_AddItemToArray(s_history, a);
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

/* ---------------- 语音合成：PCM 流经回调交付给使用方播放 ---------------- */
int llm_tts_play(const char *text, int sample_rate, llm_pcm_sink_t sink, void *ctx)
{
    if (!sink) return -1;

    cJSON *body = cJSON_CreateObject();
    cJSON_AddStringToObject(body, "model", TTS_MODEL);
    cJSON_AddStringToObject(body, "input", text);
    cJSON_AddStringToObject(body, "voice", TTS_VOICE);
    cJSON_AddStringToObject(body, "response_format", "pcm");
    cJSON_AddNumberToObject(body, "sample_rate", sample_rate);
    char *post = cJSON_PrintUnformatted(body);
    cJSON_Delete(body);

    esp_http_client_config_t cfg = {
        .url = API_BASE "/v1/audio/speech", .method = HTTP_METHOD_POST,
        .timeout_ms = TTS_TIMEOUT_MS, .crt_bundle_attach = esp_crt_bundle_attach,
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
            if (in) {
                err = 0;
                int r;
                while ((r = esp_http_client_read(c, (char *)in, 1024)) > 0) {
                    if (sink(in, r / 2, ctx) != ESP_OK) { err = -1; break; }
                }
            }
            free(in);
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
