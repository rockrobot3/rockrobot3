/* eth 实现：本板未接 PHY，接口预留（见 include/eth.h） */
#include "eth.h"
#include <stdbool.h>
#include "esp_log.h"

static const char *TAG = "eth";
static bool s_started;

void eth_start(void)
{
    if (s_started) return;
    s_started = true;
    ESP_LOGI(TAG, "以太网：未接 PHY（LAN8720 等），接口已预留");
}

bool eth_up(void)
{
    return false;   /* 无 PHY 硬件 */
}
