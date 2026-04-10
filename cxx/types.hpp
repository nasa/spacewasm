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


#ifndef SPACEWASM_TYPES_HPP
#define SPACEWASM_TYPES_HPP

namespace wasm
{
    enum TypeKind
    {
        WASM_TYPE_I32,
        WASM_TYPE_I64,
        WASM_TYPE_F32,
        WASM_TYPE_F64,
        WASM_TYPE_REF,
    };

    struct Type
    {
        TypeKind kind;
    };

    struct TypeSection
    {
        
    };
}

#endif //SPACEWASM_TYPES_HPP
