idf_component_register(SRCS "src/panel.c"
                       INCLUDE_DIRS "include"
                       PRIV_INCLUDE_DIRS "src"
                       REQUIRES esp_http_server esp_wifi esp_timer esp_system esp_app_format
                                audio relay wifi applog data)
