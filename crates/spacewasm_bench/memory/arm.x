/*
    Portions of this file were derived from <https://github.com/rust-embedded/cortex-m>
    and the cortex-m and cortex-m-rt crates developed by the Rust Embedded community.

*/

MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 1M
  RAM   : ORIGIN = 0x20000000, LENGTH = 128K
}
