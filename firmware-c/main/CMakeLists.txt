idf_component_register(SRCS "main.c" "app_wifi.c" "app_audio.c"
                    INCLUDE_DIRS "."
                    REQUIRES esp_wifi esp_event esp_netif nvs_flash
                             esp_http_client esp_timer
                             esp_driver_i2s esp_driver_gpio
                             llm)
