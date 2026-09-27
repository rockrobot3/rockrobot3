idf_component_register(SRCS "src/asr.c"
                       INCLUDE_DIRS "include"
                       PRIV_INCLUDE_DIRS "src"
                       REQUIRES esp_http_client mbedtls espressif__cjson)
