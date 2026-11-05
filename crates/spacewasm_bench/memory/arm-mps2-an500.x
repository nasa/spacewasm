/*
    Memory layout for the Cortex-M7 MPS2 AN500 board (QEMU `mps2-an500`).

    Portions of this file were derived from <https://github.com/rust-embedded/cortex-m>
    and the cortex-m and cortex-m-rt crates developed by the Rust Embedded community.

*/

MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 4M /* ZBT SSRAM1 */
  RAM   : ORIGIN = 0x20000000, LENGTH = 4M /* ZBT SSRAM 2&3 */
}
