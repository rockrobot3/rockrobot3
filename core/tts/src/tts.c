/* tts 模块实现：云端语音合成，PCM 流回调交付（从原 core/llm 拆出） */
#include "tts.h"
#include "tts_config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "esp_http_client.h"
#include "esp_crt_bundle.h"
#include "cJSON.h"
#include "esp_log.h"

static const char *TAG = "tts";

int tts_play(const char *text, int sample_rate, tts_pcm_sink_t sink, void *ctx)
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

    char auth[96];
    snprintf(auth, sizeof(auth), "Bearer %s", API_KEY);
    esp_http_client_set_header(c, "Authorization", auth);
    esp_http_client_set_header(c, "Content-Type", "application/json");

    int plen = strlen(post);
    int err = -1;
    if (esp_http_client_open(c, plen) == ESP_OK &&
        esp_http_client_write(c, post, plen) == plen) {
        esp_http_client_fetch_headers(c);
        if (esp_http_client_get_status_code(c) == 200) {
            err = 0;
            static int16_t buf[1024];        /* 网络回调上下文，静态缓冲安全 */
            while (1) {
                int r = esp_http_client_read(c, (char *)buf, sizeof(buf));
                if (r <= 0) break;
                if (sink(buf, r / 2, ctx) != ESP_OK) { err = -2; break; }
            }
        } else {
            ESP_LOGE(TAG, "合成接口 HTTP %d", esp_http_client_get_status_code(c));
        }
    }
    esp_http_client_cleanup(c);
    free(post);
    return err;
}
