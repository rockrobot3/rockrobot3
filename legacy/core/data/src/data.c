/* data 模块实现：RAM 缓存 + NVS 持久化 */
#include "data.h"
#include "data_config.h"
#include <stdio.h>
#include "nvs_flash.h"
#include "nvs.h"

static const char *s_names[DATA_COUNT] = {
    "boots", "chat_rounds", "asr_ok", "asr_fail",
    "llm_fail", "tts_fail", "relay_on", "relay_off", "errors",
};
static uint32_t s_val[DATA_COUNT];
static nvs_handle_t s_nvs;

void data_init(void)
{
    if (nvs_open(DATA_NVS_NS, NVS_READWRITE, &s_nvs) != ESP_OK) return;
    for (int k = 0; k < DATA_COUNT; k++) {
        uint32_t v = 0;
        nvs_get_u32(s_nvs, s_names[k], &v);    /* 没有就用 0 */
        s_val[k] = v;
    }
    s_val[DATA_BOOTS]++;
    nvs_set_u32(s_nvs, s_names[DATA_BOOTS], s_val[DATA_BOOTS]);
    nvs_commit(s_nvs);
}

void data_inc(data_key_t k)
{
    if (k >= DATA_COUNT || !s_nvs) return;
    s_val[k]++;
    nvs_set_u32(s_nvs, s_names[k], s_val[k]);
    nvs_commit(s_nvs);
}

uint32_t data_get(data_key_t k)
{
    return (k < DATA_COUNT) ? s_val[k] : 0;
}

int data_get_json(char *out, int sz)
{
    int o = 0;
    o += snprintf(out + o, sz - o, "{");
    for (int k = 0; k < DATA_COUNT; k++)
        o += snprintf(out + o, sz - o, "%s\"%s\":%lu",
                      k ? "," : "", s_names[k], (unsigned long)s_val[k]);
    snprintf(out + o, sz - o, "}");
    return o;
}
