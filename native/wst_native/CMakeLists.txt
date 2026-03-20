cmake_minimum_required(VERSION 3.15)
project(wst_native C)

add_library(wst_native STATIC
    src/wst_native.c
)

target_include_directories(wst_native PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/../include
)
