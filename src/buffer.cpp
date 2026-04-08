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


#include "buffer.hpp"

#include "string.hpp"

namespace wasm
{
    Buffer::Buffer(const U8* data, const WSizeType size)
        : m_data(data), m_size(size), m_offset(0)
    {
        FW_ASSERT(data != nullptr);
        FW_ASSERT(size != 0);
    }

    Buffer::Status Buffer::read_u7(U8& out)
    {
        U64 value;
        const auto status = read_leb128_unsigned(value, 7);
        out = static_cast<U8>(value);
        return status;
    }

    Buffer::Status Buffer::read_i7(I8& out)
    {
        I64 value;
        const auto status = read_leb128_signed(value, 7);
        out = static_cast<I8>(value);
        return status;
    }

    Buffer::Status Buffer::read_u32(U32& out)
    {
        U64 value;
        const auto status = read_leb128_unsigned(value, 32);
        out = static_cast<U32>(value);
        return status;
    }

    Buffer::Status Buffer::read_i32(I32& out)
    {
        I64 value;
        const auto status = read_leb128_signed(value, 32);
        out = static_cast<I32>(value);
        return status;
    }

    Buffer::Status Buffer::read_u64(U64& out)
    {
        return read_leb128_unsigned(out, 64);
    }

    Buffer::Status Buffer::read_i64(I64& out)
    {
        return read_leb128_signed(out, 64);
    }

    Buffer::Status Buffer::read_utf8(String& out)
    {
        U32 length;

        auto status = read_u32(length);
        if (status != OK)
        {
            return status;
        }

        if (length >= WASM_STRING_LENGTH)
        {
            return INVALID_LENGTH;
        }

        if (m_offset + length >= m_size)
        {
            return OVERFLOW;
        }

        out.set(reinterpret_cast<const char*>(&m_data[m_offset]), length);
        m_offset += length;

        return OK;
    }

    Buffer::Status Buffer::read_leb128_signed(I64& out, const U32 max_num_bits)
    {
        Status status = UNDERFLOW;

        I64 value = 0;
        U32 shift = 0;

        while (m_offset < m_size)
        {
            const U8 byte = m_data[m_offset];
            m_offset++;

            value |= static_cast<I64>(byte & 0x7F) << shift;
            shift += 7;

            if ((byte & 0x80) == 0)
            {
                status = OK;

                // Perform sign extension if negative
                if (byte & 0x40 && shift < 64)
                {
                    value |= static_cast<I64>(-1ULL << shift);
                }

                break;
            }

            if (shift > max_num_bits)
            {
                status = OVERFLOW;
            }
        }

        out = value;
        return status;
    }

    Buffer::Status Buffer::read_leb128_unsigned(U64& out, const U32 max_num_bits)
    {
        Status status = UNDERFLOW;

        U64 value = 0;
        U32 shift = 0;

        while (m_offset < m_size)
        {
            const U8 byte = m_data[m_offset];
            m_offset++;

            value |= static_cast<U64>(byte & 0x7F) << shift;
            shift += 7;

            if ((byte & 0x80) == 0)
            {
                status = OK;
                break;
            }

            if (shift > max_num_bits)
            {
                status = OVERFLOW;
            }
        }

        out = value;
        return status;
    }
}
