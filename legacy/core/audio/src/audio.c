#include "audio.h"
#include "audio_config.h"
#include <math.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/i2s_std.h"
#include "esp_err.h"

#ifndef M_PI
#define M_PI 3.14159265358979f
#endif

static i2s_chan_handle_t s_tx, s_rx;
static int32_t s_dc;           /* 麦克风直流偏移估计 */
static bool s_mic_en = true;   /* 麦克风静音开关（默认开） */

void audio_init(void)
{
    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_0, I2S_ROLE_MASTER);
    chan_cfg.dma_frame_num = AUDIO_CHUNK;
    chan_cfg.dma_desc_num = 10;   /* 缓冲 160ms */
    chan_cfg.auto_clear = true;   /* 数据断流时 DMA 发静音；否则会循环重放最后一段（卡字） */
    ESP_ERROR_CHECK(i2s_new_channel(&chan_cfg, &s_tx, &s_rx));

    i2s_std_config_t std_cfg = {
        .clk_cfg  = I2S_STD_CLK_DEFAULT_CONFIG(AUDIO_SAMPLE_RATE),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_32BIT, I2S_SLOT_MODE_MONO),
        .gpio_cfg = { .mclk = I2S_GPIO_UNUSED, .bclk = I2S_BCLK, .ws = I2S_WS,
                      .dout = I2S_DOUT, .din = I2S_DIN,
                      .invert_flags = { 0, 0, 0 } },
    };
    ESP_ERROR_CHECK(i2s_channel_init_std_mode(s_tx, &std_cfg));
    ESP_ERROR_CHECK(i2s_channel_init_std_mode(s_rx, &std_cfg));
    ESP_ERROR_CHECK(i2s_channel_enable(s_tx));
    ESP_ERROR_CHECK(i2s_channel_enable(s_rx));
}

int audio_read(int32_t *buf, int max_bytes, size_t *bytes_read, int timeout_ms)
{
    *bytes_read = 0;
    if (!s_mic_en) {                      /* 静音：等到超时再返回 0 字节，调用方自然空转 */
        vTaskDelay(pdMS_TO_TICKS(20));
        return ESP_OK;
    }
    return i2s_channel_read(s_rx, buf, max_bytes, bytes_read, timeout_ms);
}

int audio_write(const void *data, size_t bytes, int timeout_ms)
{
    size_t bw;
    return i2s_channel_write(s_tx, data, bytes, &bw, timeout_ms);
}

void audio_play_tone(int freq, int ms)
{
    int n = AUDIO_SAMPLE_RATE * ms / 1000;
    int32_t out[AUDIO_CHUNK];
    for (int i = 0; i < n; i += AUDIO_CHUNK) {
        int m = (AUDIO_CHUNK < n - i) ? AUDIO_CHUNK : n - i;
        for (int j = 0; j < m; j++)
            out[j] = (int32_t)(sinf(2 * M_PI * freq * (i + j) / AUDIO_SAMPLE_RATE) * 12000) << 8;
        audio_write(out, m * 4, 1000);
    }
}

int audio_mic_process(int32_t raw, int16_t *out)
{
    int32_t x = raw >> 16;
    s_dc += (x - s_dc) >> 8;          /* 慢速跟踪直流偏移（约10Hz高通） */
    int32_t y = x - s_dc;
    int peak = (y < 0) ? -y : y;      /* 峰值用原始幅度（VAD 判断用） */
    int32_t g = y << REC_GAIN;        /* 上传的音频加增益 */
    if (g > 32000) g = 32000;
    else if (g < -32000) g = -32000;
    *out = (int16_t)g;
    return peak;
}

void audio_mic_set_enable(bool en)
{
    s_mic_en = en;
}

bool audio_mic_enabled(void)
{
    return s_mic_en;
}
