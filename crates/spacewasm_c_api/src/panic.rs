//! `#[panic_handler]` for the standalone C library build.

use core::panic::PanicInfo;

unsafe extern "C" {
    /// Handle a spacewasm Rust panic. This function should not return
    ///
    /// # Arguments
    ///
    /// * `filename`: Filename where panic occurred (not null terminated)
    /// * `filename_len`: Length of filename (could be 0)
    /// * `line`: Line number where panic occured
    /// * `msg`: Panic message if it could be extracted (not null terminated)
    /// * `len`: Length of panic message (zero if empty)
    ///
    /// returns: !
    fn spacewasm_panic(
        filename: *const u8,
        filename_len: usize,
        line: u32,
        msg: *const u8,
        len: usize,
    ) -> !;
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let msg = info.message().as_str().unwrap_or("spacewasm: panic");
    let (filename, line) = if let Some(location) = info.location() {
        (location.file(), location.line())
    } else {
        ("", 0)
    };

    unsafe {
        spacewasm_panic(
            filename.as_ptr(),
            filename.len(),
            line,
            msg.as_ptr(),
            msg.len(),
        )
    }
}

/// Stub for the exception-handling personality routine. The precompiled `core`
/// is built for unwinding and references `rust_eh_personality` even though we
/// compile with `panic = "abort"`, so a C consumer linking the staticlib would
/// otherwise see an undefined symbol. Under `panic = "abort"` it is never
/// actually invoked; providing it here keeps the archive self-contained.
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}
