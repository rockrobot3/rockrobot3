idf_component_register(SRCS "src/asr.c"
                       INCLUDE_DIRS "include"
                       PRIV_INCLUDE_DIRS "src"
                       REQUIRES esp_http_client mbedtls espressif__cjson)

# 密钥注入：缺失或为空即失败，不产出"能编译但上线 401"的固件
if(NOT DEFINED ENV{SILICONFLOW_API_KEY} OR "$ENV{SILICONFLOW_API_KEY}" STREQUAL "")
    message(FATAL_ERROR
        "SILICONFLOW_API_KEY 未设置。构建前执行：export SILICONFLOW_API_KEY=sk-… "
        "（PowerShell: $env:SILICONFLOW_API_KEY=\"sk-…\"）")
endif()
target_compile_definitions(${COMPONENT_LIB} PRIVATE
                           SILICONFLOW_API_KEY="$ENV{SILICONFLOW_API_KEY}")
