idf_component_register(SRCS "src/audio.c"
                       INCLUDE_DIRS "include"
                       PRIV_INCLUDE_DIRS "src"
                       REQUIRES esp_driver_i2s esp_driver_gpio)
