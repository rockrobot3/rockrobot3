# For more information about build system see
# https://docs.espressif.com/projects/esp-idf/en/latest/api-guides/build-system.html
# The following five lines of boilerplate have to be in your project's
# CMakeLists in this exact order for cmake to work correctly
cmake_minimum_required(VERSION 3.16)

# 版本号：从 git tag 自动生成（如 v1.1 或 v1.1-3-gabcdef），无 tag/非 git 环境回退 0.0.0
execute_process(COMMAND git describe --tags --always --dirty
                WORKING_DIRECTORY ${CMAKE_CURRENT_LIST_DIR}/..
                OUTPUT_VARIABLE PROJECT_VER
                OUTPUT_STRIP_TRAILING_WHITESPACE
                ERROR_QUIET
                RESULT_VARIABLE _ver_ret)
if(NOT _ver_ret EQUAL 0 OR "${PROJECT_VER}" STREQUAL "")
    set(PROJECT_VER "0.0.0")
endif()

# 复用 RockRobot3 的独立模块（core/；通信链路统一在 core/link/ 下）
list(APPEND EXTRA_COMPONENT_DIRS
     "${CMAKE_CURRENT_LIST_DIR}/../core/llm"
     "${CMAKE_CURRENT_LIST_DIR}/../core/asr"
     "${CMAKE_CURRENT_LIST_DIR}/../core/tts"
     "${CMAKE_CURRENT_LIST_DIR}/../core/link/wifi"
     "${CMAKE_CURRENT_LIST_DIR}/../core/link/btctl"
     "${CMAKE_CURRENT_LIST_DIR}/../core/link/nearlink"
     "${CMAKE_CURRENT_LIST_DIR}/../core/link/eth"
     "${CMAKE_CURRENT_LIST_DIR}/../core/audio"
     "${CMAKE_CURRENT_LIST_DIR}/../core/relay"
     "${CMAKE_CURRENT_LIST_DIR}/../core/panel"
     "${CMAKE_CURRENT_LIST_DIR}/../core/applog"
     "${CMAKE_CURRENT_LIST_DIR}/../core/data")

include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(rockrobot3)
