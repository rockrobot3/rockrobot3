/* nearlink 实现：本平台无星闪硬件，接口预留（见 include/nearlink.h） */
#include "nearlink.h"
#include <stdbool.h>
#include "esp_log.h"

static const char *TAG = "nearlink";
static bool s_started;

void nearlink_start(void)
{
    if (s_started) return;
    s_started = true;
    ESP_LOGI(TAG, "星闪 NearLink：本硬件不支持（接口已预留）");
}

bool nearlink_up(void)
{
    return false;   /* 无射频硬件 */
}
