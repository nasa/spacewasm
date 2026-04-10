cmake_minimum_required(VERSION 3.20)

project(spacewasm)
enable_language(CXX)

include_directories(config)

add_library(
        spacewasm
        src/types.cpp
        src/types.hpp
        src/values.cpp
        src/values.hpp
        src/buffer.cpp
        src/buffer.hpp
        src/string.cpp
        src/string.hpp
        src/module.cpp
        src/module.hpp
)
