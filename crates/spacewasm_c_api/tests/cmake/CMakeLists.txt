####
# CMake integration fixture.
#
# Consumes `spacewasm_c_api` exactly the way `crates/spacewasm_c_api/README.md`
# tells an integrator to: `add_subdirectory()` plus a link against `spacewasm`.
# The programs built here are the same ones `spacewasm_c_example`'s `c_abi.rs`
# drives through cargo, so this adds coverage of the CMake path without adding
# test code. Both are self-contained (module bytes are embedded, no file I/O),
# so unlike the rest of the suite this needs no WABT.
####
cmake_minimum_required(VERSION 3.12)
project(spacewasm_cmake_integration C)

add_subdirectory(../.. spacewasm_c_api)

set(SPACEWASM_C_EXAMPLES ${CMAKE_CURRENT_SOURCE_DIR}/../../../spacewasm_c_example/examples)

foreach(prog ctest ctest_suite)
  add_executable(${prog} ${SPACEWASM_C_EXAMPLES}/${prog}.c)
  target_link_libraries(${prog} PRIVATE spacewasm)
endforeach()
