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


#include "string.hpp"

#include <cstring>

namespace wasm
{
    String::String()
        : m_length(0), m_data()
    {
    }

    String::String(const String& other)
        : m_length(other.m_length), m_data()
    {
        memcpy(m_data, other.m_data, m_length);
    }

    String::String(const char* other, const U16 length)
        : m_length(length), m_data()
    {
        FW_ASSERT(length <= sizeof(m_data));
        memcpy(m_data, other, length);
    }

    void String::set(const char* other, const U16 length)
    {
        FW_ASSERT(length <= sizeof(m_data));
        memcpy(m_data, other, length);
        m_length = length;
    }
}
