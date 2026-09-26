idf_component_register(SRCS "src/relay.c"
                       INCLUDE_DIRS "include"
                       PRIV_INCLUDE_DIRS "."
                       REQUIRES esp_driver_gpio)
