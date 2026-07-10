# SpaceWasm IR Specification

This document describes the application binary interface (ABI) of the
SpaceWasm intermediate format (IR). IR is the format that can be directly
interpreted or compiled by a JIT and executed. IR differs from Wasm bytecode
as it strips away the structure of the instructions and resolves references
and indexes.

## Overview

The SpaceWasm IR uses fixed-width encodings to allow for fast execution and
aligned storage and retrieval. SpaceWasm IR uses 16-bit words to encode instructions.
Instructions may use 1-3 words depending on their operands. For this reason, SpaceWasm
instructions typically take more memory than their raw Wasm counterparts.

## Instructions

Wasm instructions represented in the IR fall in the following categories:

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

## Limitations

Are several limitations enforced by the interpreter that generally stem
from tuned fixed-width integer constraints imposed by the IR design and
datastructures within the implementation.

> [!NOTE]
> We have found that these limitations are generally not highly restrictive and even got a Pyiodide interpreter running inside SpaceWasm.

### Module & Store Limits

- **Modules in store**: Maximum 256 modules
- **Host modules**: Maximum 256 host modules
- **Function parameters**: Maximum 255 32-bit words
- **Local variables**: Maximum 65,535 32-bit words total per function

### Linear Memory

These follow the WebAssembly 1.0 specification and are validated at decode time.

- **Page size**: 64 KiB (65,536 bytes) — the standard Wasm linear-memory page size. The
  [custom-page-sizes proposal](https://github.com/WebAssembly/custom-page-sizes) is planned but not yet supported,
  so this size is fixed.
- **Maximum pages**: 65,536 pages (4 GiB) per memory. A declared `min` or `max` above this is rejected.

### IR Code Pages

These pages hold the compiled IR (not raw Wasm bytecode) and are distinct from linear-memory pages. This limit comes
from the encoding of program counters and the design choices of the IR.

- **Code pages**: Configurable via generic parameter `MAX_CODE_PAGES`, typically set at module instantiation
- **Page size**: 256 16-bit words (512 bytes)
- **Maximum page index**: 24-bit (16,777,216 pages)
- **Word offset in page**: 8-bit (0-255)

### Control Flow

- **Nesting depth**: Configurable via generic parameter `MAX_CONTROL_FRAMES` (blocks/loops/if-else)
- **Value stack**: Configurable via generic parameter `MAX_STACK_DEPTH`, values per function
- **Label jumps**: 22-bit signed offset (±2,097,151 instructions)
- **Stack truncation depth**: Maximum 255 32-bit words per label jump

### Instruction Encoding

- **8-bit or 16-bit indexes**: 0-65,535
- **8-bit or 32-bit immediate**: 0-254 inline, 255+ extended
- **8-bit or 64-bit immediate**: 0-254 inline, 255+ extended
