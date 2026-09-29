#include "relay.h"
#include "relay_config.h"
#include "driver/gpio.h"

static bool s_state;   /* 软件状态：REL 针悬空/未接时引脚电平不可信，不回读引脚 */

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
    s_state = false;
}

void relay_set(bool on)
{
    gpio_set_level(RELAY_GPIO, on ? 1 : 0);
    s_state = on;
}

bool relay_get(void)
{
    return s_state;
}
