idf_component_register(SRCS "src/relay.c"
                       INCLUDE_DIRS "include"
                       PRIV_INCLUDE_DIRS "src"
                       REQUIRES esp_driver_gpio)
