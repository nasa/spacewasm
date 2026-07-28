cmake_minimum_required(VERSION 3.12)
project(spacewasm_c_api C)
include(ExternalProject)

set(SPACEWASM_TARGET "" CACHE STRING "Target triple for Rust compiler")

if(SPACEWASM_TARGET STREQUAL "")
  execute_process(COMMAND rustc -vV OUTPUT_VARIABLE RUSTC_VERSION)
  string(REGEX MATCH "host: ([^ \n]*)" RUSTC_HOST ${RUSTC_VERSION})
  string(STRIP ${CMAKE_MATCH_1} RUSTC_HOST_TARGET)
  set(SPACEWASM_TARGET ${RUSTC_HOST_TARGET})
endif()

if(CMAKE_BUILD_TYPE STREQUAL "Debug")
  set(SPACEWASM_BUILD_TYPE "debug")
  set(SPACEWASM_BUILD_TYPE_FLAG "")
else()
  set(SPACEWASM_BUILD_TYPE "release")
  set(SPACEWASM_BUILD_TYPE_FLAG "--release")
endif()

if(DEFINED ENV{CARGO_TARGET_DIR})
  set(CARGO_TARGET_DIR $ENV{CARGO_TARGET_DIR})
else()
  set(CARGO_TARGET_DIR ${CMAKE_CURRENT_BINARY_DIR}/spacewasm)
endif()

set(SPACEWASM_TARGET_DIR ${CARGO_TARGET_DIR}/${SPACEWASM_TARGET}/${SPACEWASM_BUILD_TYPE})
set(SPACEWASM_STATIC_FILES ${SPACEWASM_TARGET_DIR}/libspacewasm_c_api.a)

find_program(SPACEWASM_CARGO_BINARY cargo)
if(NOT SPACEWASM_CARGO_BINARY)
    message(FATAL_ERROR "cargo not found")
endif()

ExternalProject_Add(
    spacewasm-c-api-crate
    DOWNLOAD_COMMAND ""
    CONFIGURE_COMMAND ""
    BUILD_COMMAND
        ${CMAKE_COMMAND} -E env CARGO_TARGET_DIR=${CARGO_TARGET_DIR}
        ${SPACEWASM_CARGO_BINARY} build
          --target ${SPACEWASM_TARGET}
          --package spacewasm_c_api
          ${SPACEWASM_BUILD_TYPE_FLAG}
    USES_TERMINAL_BUILD TRUE
    BINARY_DIR ${CMAKE_CURRENT_SOURCE_DIR}
    BUILD_ALWAYS ON
    BUILD_BYPRODUCTS ${SPACEWASM_STATIC_FILES}
    INSTALL_COMMAND "")

add_library(spacewasm INTERFACE)
add_dependencies(spacewasm spacewasm-c-api-crate)

target_link_libraries(spacewasm INTERFACE ${SPACEWASM_STATIC_FILES})

target_include_directories(spacewasm
  INTERFACE
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>
)
