use semihosting;
mod alloc;

pub use riscv_rt::entry;

const CLOCK_HZ: u32 = 1_000_000_000; // 1 GHz

pub fn clock_setup() {
    // nothing to do here
}

pub fn clock_get_ms() -> i64 {
    let ticks = riscv::register::time::read() as f64;
    (ticks / ((CLOCK_HZ / 1000) as f64)) as i64
}

pub fn exit(code: i32) -> ! {
    semihosting::process::exit(code);
}

pub fn init_allocator() {
    alloc::init();
}
