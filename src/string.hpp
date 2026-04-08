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


#ifndef SPACEWASM_STRING_HPP
#define SPACEWASM_STRING_HPP

#include "wasm_config.hpp"

namespace wasm
{
    struct String
    {
        String();
        String(const String& other);
        String(const char* other, U16 length);

        void set(const char* other, U16 length);

    private:
        U16 m_length;
        U8 m_data[WASM_STRING_LENGTH];
    };
}

#endif //SPACEWASM_STRING_HPP
