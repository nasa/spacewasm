use rtic_monotonics::systick::prelude::*;

pub use cortex_m_rt::entry;

mod alloc;

const CLOCK_HZ: u32 = 1_000_000_000; // 1 GHz

systick_monotonic!(Mono, 1_000);

pub fn clock_setup() {
    let p = cortex_m::Peripherals::take().unwrap();
    Mono::start(p.SYST, CLOCK_HZ);
}

pub fn clock_get_ms() -> i64 {
    Mono::now().ticks() as i64
}

pub fn exit(code: i32) -> ! {
    semihosting::process::exit(code);
}

pub fn init_allocator() {
    alloc::init();
}
