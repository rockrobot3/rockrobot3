idf_component_register(SRCS "llm.c"
                       INCLUDE_DIRS "include"
                       REQUIRES esp_http_client mbedtls espressif__cjson)
