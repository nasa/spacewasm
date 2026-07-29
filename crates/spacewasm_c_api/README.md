# SpaceWasm C API

## CMake Integration

SpaceWasm can be easily included in your CMake project as well. Simply point your existing project to the
`spacewasm_c_api` directory like so:

```cmake
add_subdirectory(spacewasm/crates/spacewasm_c_api)
target_link_libraries(your_target PRIVATE spacewasm)
```

Cross compiling simply requires setting `SPACEWASM_TARGET` to the appropriate Rust target triple, and
requires the appropriate Rust toolchain and C toolchain installed.

By default, release binaries are built unless `CMAKE_BUILD_TYPE` is set to `Debug`.

