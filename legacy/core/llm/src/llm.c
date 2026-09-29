/* llm 模块实现：大模型多轮对话（带历史记忆），从原混合实现中拆出 */
#include "llm.h"
#include "llm_config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "esp_http_client.h"
#include "esp_crt_bundle.h"
#include "cJSON.h"
#include "esp_log.h"

static const char *TAG = "llm";

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
    char auth[96];
    snprintf(auth, sizeof(auth), "Bearer %s", API_KEY);
    esp_http_client_set_header(c, "Authorization", auth);
    esp_http_client_set_header(c, "Content-Type", "application/json");
    int status = -1;
    if (esp_http_client_open(c, body_len) == ESP_OK && write_all(c, body, body_len) == 0) {
        esp_http_client_fetch_headers(c);
        int total = 0;
        while (total < resp_sz - 1) {
            int r = esp_http_client_read(c, resp + total, resp_sz - 1 - total);
            if (r <= 0) break;
            total += r;
        }
        resp[total] = 0;
        status = esp_http_client_get_status_code(c);
    }
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
        ESP_LOGE(TAG, "大模型接口 HTTP %d: %s", status, resp);
    }
    free(post);
    free(resp);
    return err;
}
