/*
    Places the embedded coremark wasm module in its own flash section so it is
    not counted towards `.rodata`. The module is benchmark input, not part of
    SpaceWasm itself.
*/

SECTIONS
{
  .wasm_module : ALIGN(4)
  {
    KEEP(*(.wasm_module .wasm_module.*));
  } > FLASH
}
INSERT AFTER .rodata;
