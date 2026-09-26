#include "app_audio.h"
#include "config.h"
#include <math.h>
#include "driver/i2s_std.h"
#include "driver/gpio.h"
#include "esp_err.h"

#ifndef M_PI
#define M_PI 3.14159265358979f
#endif

static i2s_chan_handle_t s_tx, s_rx;
static int32_t s_dc;           /* 麦克风直流偏移估计 */

void audio_init(void)
{
    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_0, I2S_ROLE_MASTER);
    chan_cfg.dma_frame_num = CHUNK;
    chan_cfg.dma_desc_num = 10;   /* 缓冲 160ms */
    ESP_ERROR_CHECK(i2s_new_channel(&chan_cfg, &s_tx, &s_rx));

    i2s_std_config_t std_cfg = {
        .clk_cfg  = I2S_STD_CLK_DEFAULT_CONFIG(SAMPLE_RATE),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_32BIT, I2S_SLOT_MODE_MONO),
        .gpio_cfg = { .mclk = I2S_GPIO_UNUSED, .bclk = GPIO_NUM_4, .ws = GPIO_NUM_5,
                      .dout = GPIO_NUM_25, .din = GPIO_NUM_18,
                      .invert_flags = { 0, 0, 0 } },
    };
    ESP_ERROR_CHECK(i2s_channel_init_std_mode(s_tx, &std_cfg));
    ESP_ERROR_CHECK(i2s_channel_init_std_mode(s_rx, &std_cfg));
    ESP_ERROR_CHECK(i2s_channel_enable(s_tx));
    ESP_ERROR_CHECK(i2s_channel_enable(s_rx));
}

int audio_read(int32_t *buf, int max_bytes, size_t *bytes_read, int timeout_ms)
{
    return i2s_channel_read(s_rx, buf, max_bytes, bytes_read, timeout_ms);
}

int audio_write(const void *data, size_t bytes, int timeout_ms)
{
    size_t bw;
    return i2s_channel_write(s_tx, data, bytes, &bw, timeout_ms);
}

void audio_play_tone(int freq, int ms)
{
    int n = SAMPLE_RATE * ms / 1000;
    int32_t out[CHUNK];
    for (int i = 0; i < n; i += CHUNK) {
        int m = (CHUNK < n - i) ? CHUNK : n - i;
        for (int j = 0; j < m; j++)
            out[j] = (int32_t)(sinf(2 * M_PI * freq * (i + j) / SAMPLE_RATE) * 12000) << 8;
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

void relay_init(void)
{
    gpio_config_t io = {
        .pin_bit_mask = 1ULL << RELAY_GPIO,
        .mode = GPIO_MODE_OUTPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    gpio_config(&io);
    gpio_set_level(RELAY_GPIO, 0);
}

void relay_set(bool on)
{
    gpio_set_level(RELAY_GPIO, on ? 1 : 0);
}
