pub fn clock_setup() {
    // nothing to do here
}

pub fn clock_get_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

pub fn exit(code: i32) -> ! {
    std::process::exit(code);
}

pub fn init_allocator() {
    // nothing to do here
}
