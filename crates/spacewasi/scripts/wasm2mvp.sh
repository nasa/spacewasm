#!/usr/bin/env bash

if ! command -v wasm-opt --version >/dev/null 2>&1
then
    echo "please install wasm-opt to use this command"
    exit 1
fi

if [ "$#" -ne 2 ]; then
    if [ "$#" -ne 1 ]; then
        echo "usage: $0 input.wasm [output.wasm]"
        exit 1
    fi
fi

wasm-opt \
    --llvm-memory-copy-fill-lowering \
    --signext-lowering \
    --disable-bulk-memory \
    --llvm-nontrapping-fptoint-lowering \
    --disable-multivalue \
    --disable-simd \
    $1 \
    -o ${2:-$1}

exit $?