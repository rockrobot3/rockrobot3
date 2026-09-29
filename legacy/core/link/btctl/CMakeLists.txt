idf_component_register(SRCS "src/btctl.c"
                       INCLUDE_DIRS "include"
                       PRIV_INCLUDE_DIRS "src"
                       REQUIRES bt nvs_flash esp_timer esp_app_format
                                wifi audio relay applog data)
