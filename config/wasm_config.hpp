/**
 * SpaceWASM
 *
 * @author Andrei Tumbar
 *
 * Copyright 2026
 * California Institute of Technology.
 * ALL RIGHTS RESERVED.
 * U.S. Government sponsorship acknowledged.
 */


#ifndef SPACEWASM_WASM_CONFIG_HPP
#define SPACEWASM_WASM_CONFIG_HPP

#include <cinttypes>
#include <cassert>

using U8 = uint8_t;
using I8 = int8_t;
using U16 = uint16_t;
using I16 = int16_t;
using U32 = uint32_t;
using I32 = int32_t;
using U64 = uint64_t;
using I64 = int64_t;

using F32 = float;
using F64 = double;

using WSizeType = size_t;

#define FW_ASSERT(expr) assert(expr)

#define WASM_STRING_LENGTH (64)

#endif //SPACEWASM_WASM_CONFIG_HPP
