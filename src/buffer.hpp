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


#ifndef SPACEWASM_BUFFER_HPP
#define SPACEWASM_BUFFER_HPP

#include "wasm_config.hpp"

namespace wasm
{
    struct String;

    struct Buffer
    {
        enum Status
        {
            OK,
            OVERFLOW,
            UNDERFLOW,
            INVALID_LENGTH,

        };

        Buffer(const U8* data, WSizeType size);

        Status read_u7(U8& out);
        Status read_i7(I8& out);

        Status read_u32(U32& out);
        Status read_i32(I32& out);

        Status read_u64(U64& out);
        Status read_i64(I64& out);

        Status read_utf8(String& out);

    private:

        Status read_leb128_signed(I64& out, U32 max_num_bits);
        Status read_leb128_unsigned(U64& out, U32 max_num_bits);

        const U8* m_data;
        WSizeType m_size;

        WSizeType m_offset;
    };
}

#endif //SPACEWASM_BUFFER_HPP
