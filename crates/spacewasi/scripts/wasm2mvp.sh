#!/usr/bin/env bash

if ! command -v wasm-opt >/dev/null 2>&1
then
    echo "please install wasm-opt to use this command"
    exit 1
fi

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 input.wasm [output.wasm]"
    exit 1
fi

wasm-opt \
    --llvm-memory-copy-fill-lowering \
    --signext-lowering \
    --disable-bulk-memory \
    --llvm-nontrapping-fptoint-lowering \
    --disable-multivalue \
    --disable-simd \
    "$1" \
    -o "${2:-$1}"

exit $?