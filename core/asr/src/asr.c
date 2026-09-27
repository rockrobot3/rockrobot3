/* asr 模块实现：流式语音识别（固定长度上传，说完提前结束+补零）
 * 从原 core/llm 拆出。服务端不支持 chunked 分块（实测 500），
 * 正确做法=固定 Content-Length 声明总长、WAV 头同长、剩余补静音零。 */
#include "asr.h"
#include "asr_config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "esp_http_client.h"
#include "esp_crt_bundle.h"
#include "cJSON.h"
#include "esp_log.h"

static const char *TAG = "asr";

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

static void put32(uint8_t *p, uint32_t v) { p[0]=v; p[1]=v>>8; p[2]=v>>16; p[3]=v>>24; }
static void put16(uint8_t *p, uint16_t v) { p[0]=v; p[1]=v>>8; }

static void build_wav_header(uint8_t *b, int sample_rate, int data_len)
{
    memcpy(b, "RIFF", 4);
    put32(b + 4, 36 + data_len);
    memcpy(b + 8, "WAVEfmt ", 8);
    put32(b + 16, 16);
    put16(b + 20, 1);
    put16(b + 22, 1);
    put32(b + 24, sample_rate);
    put32(b + 28, sample_rate * 2);
    put16(b + 32, 2);
    put16(b + 34, 16);
    memcpy(b + 36, "data", 4);
    put32(b + 40, data_len);
}

#define ASR_BND "ESP32VOICE"
#define ASR_TAIL "\r\n--" ASR_BND "--\r\n"

struct asr_s {
    esp_http_client_handle_t c;
    char resp[1024];
    int sent;        /* 已发送的 PCM 字节数 */
    int max_bytes;   /* 声明的 PCM 总字节数 */
};

asr_t *asr_begin(int sample_rate, int max_seconds)
{
    asr_t *s = calloc(1, sizeof(asr_t));
    if (!s) return NULL;
    s->max_bytes = sample_rate * max_seconds * 2;

    esp_http_client_config_t cfg = {
        .url = API_BASE "/v1/audio/transcriptions", .method = HTTP_METHOD_POST,
        .timeout_ms = ASR_TIMEOUT_MS, .crt_bundle_attach = esp_crt_bundle_attach,
        .buffer_size = 2048, .buffer_size_tx = 4096,
    };
    s->c = esp_http_client_init(&cfg);
    if (!s->c) { free(s); return NULL; }
    char auth[96], ct[64], head[384];
    snprintf(auth, sizeof(auth), "Bearer %s", API_KEY);
    esp_http_client_set_header(s->c, "Authorization", auth);
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
    build_wav_header(wav, sample_rate, s->max_bytes);
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

int asr_chunk(asr_t *s, const void *data, int len)
{
    if (!s || len <= 0) return 0;
    if (write_all(s->c, (const char *)data, len) < 0) return -1;
    s->sent += len;
    return 0;
}

int asr_sent(asr_t *s)
{
    return s ? s->sent : 0;
}

void asr_abort(asr_t *s)
{
    if (!s) return;
    if (s->c) {
        esp_http_client_cleanup(s->c);
        s->c = NULL;
    }
    free(s);
}

int asr_finish(asr_t *s, char *text, int text_sz)
{
    int err = -1;
    int remain = s->max_bytes - s->sent;
    char zbuf[2048];
    memset(zbuf, 0, sizeof(zbuf));
    int ok = 1;
    while (remain > 0 && ok) {                   /* 补零到声明的总长度 */
        int m = (remain < (int)sizeof(zbuf)) ? remain : (int)sizeof(zbuf);
        if (write_all(s->c, zbuf, m) < 0) ok = 0;
        remain -= m;
    }
    if (ok && write_all(s->c, ASR_TAIL, strlen(ASR_TAIL)) == 0) {
        esp_http_client_fetch_headers(s->c);
        int total = 0;
        while (total < (int)sizeof(s->resp) - 1) {
            int r = esp_http_client_read(s->c, s->resp + total, sizeof(s->resp) - 1 - total);
            if (r <= 0) break;
            total += r;
        }
        s->resp[total] = 0;
        int status = esp_http_client_get_status_code(s->c);
        if (status == 200) {
            cJSON *j = cJSON_Parse(s->resp);
            if (j) {
                char *t = cJSON_GetStringValue(cJSON_GetObjectItem(j, "text"));
                if (t) { snprintf(text, text_sz, "%s", t); err = 0; }
                cJSON_Delete(j);
            }
        } else {
            ESP_LOGE(TAG, "识别接口 HTTP %d: %s", status, s->resp);
        }
    }
    esp_http_client_cleanup(s->c);
    s->c = NULL;
    free(s);
    return err;
}
