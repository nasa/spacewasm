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


#ifndef SPACEWASM_MODULE_HPP
#define SPACEWASM_MODULE_HPP
#include "types.hpp"

namespace wasm
{
    enum Section
    {
        SECTION_CUSTOM = 0,
        SECTION_TYPE = 1,
        SECTION_IMPORT = 2,
        SECTION_FUNCTION = 3,
        SECTION_TABLE = 4,
        SECTION_MEMORY = 5,
        SECTION_GLOBAL = 6,
        SECTION_EXPORT = 7,
        SECTION_START = 8,
        SECTION_ELEMENT = 9,
        SECTION_CODE = 10,
        SECTION_DATA = 11,
        SECTION_DATA_COUNT = 12,
        SECTION_TAG = 13,
    };

    struct Module
    {
        Module();

    private:

        TypeSection m_type;
        bool m_type_filled;


    };
}

#endif //SPACEWASM_MODULE_HPP
