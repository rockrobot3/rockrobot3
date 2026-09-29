idf_component_register(SRCS "main.c"
                    INCLUDE_DIRS "."
                    REQUIRES nvs_flash esp_timer esp_system
                             wifi btctl nearlink eth
                             audio relay panel asr llm tts applog data)
