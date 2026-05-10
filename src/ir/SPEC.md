# SpaceWASM IR Specification

This document describes the application binary interface (ABI) of the
SpaceWASM intermediate format (IR). IR is the format that can be directly
interpreted or compiled by a JIT and executed. IR differs from WASM bytecode
as it strips away the structure of the instructions and resolves references
and indexes.

## Overview

The SpaceWASM IR uses fixed-width encodings to allow for fast execution and
aligned storage and retrieval. SpaceWASM IR uses 16-bit words to encode instructions.
Instructions may use 1-3 words depending on their operands. For this reason, SpaceWASM
instructions typically take more memory than their raw WASM counterparts.

## Instructions

WASM instructions represented in the IR fall in the following categories:

1. No operand (opcode only)
   ```
   [8:opcode][8:_]
   ```
2. Memory operand
   ```
   [8:opcode][8:align]
   [16:offset_lo]
   [16:offset_hi]
   ```
3. 8/16 Operand
    - If operand < 255:
        ```
        [8:opcode][8:operand]
        ```
    - else
       ```
       [8:opcode][8=255]
       [16:operand]
       ```
4. 8/32 Operand
    - If operand < 255:
       ```
       [8:opcode][8:operand]
       ```
    - else
       ```
       [8:opcode][8=255]
       [16:operand_lo]
       [16:operand_hi]
       ```
5. 8/64 Operand
    - If operand < 255:
       ```
       [8:opcode][8:operand]
       ```
    - else
       ```
       [8:opcode][8=255]
       [16:operand_[0-15]]
       [16:operand_[16-31]]
       [16:operand_[32-47]]
       [16:operand_[48-64]]
       ```
6. Local Operand
   ```
   [8:opcode][8:ValTy]
   [16:frame_offset]
   ```
7. Global Operand
   ```
   [8:opcode][1:imported (1) or internal (0)][7:ValTy]
   [16:index]
   ```
8. Other...
