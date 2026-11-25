//! Panicking helpers that never format the error value.
//!
//! `Result::unwrap`/`expect` pass the error to the panic handler as
//! `&dyn Debug`. Even when the handler ignores it, the `Debug` impl (and much
//! of `core::fmt`) stays in the binary unless LTO can prove it is unused, which
//! it often cannot when building with custom `-C target-cpu`/`target-feature`
//! flags. These helpers panic with a static message, so no `Debug` impl is
//! referenced.

/// Extension for [`Result`] that unwraps without formatting the error.
pub trait UnwrapNoFmt<T> {
    /// Like [`Result::expect`], but the error value is not formatted.
    fn expect_nofmt(self, msg: &'static str) -> T;

    /// Like [`Result::unwrap`], but the error value is not formatted.
    fn unwrap_nofmt(self) -> T;
}

impl<T, E> UnwrapNoFmt<T> for Result<T, E> {
    #[inline]
    #[track_caller]
    fn expect_nofmt(self, msg: &'static str) -> T {
        match self {
            Ok(v) => v,
            Err(_) => fail(msg),
        }
    }

    #[inline]
    #[track_caller]
    fn unwrap_nofmt(self) -> T {
        self.expect_nofmt("called `unwrap_nofmt()` on an `Err` value")
    }
}

#[cold]
#[inline(never)]
#[track_caller]
fn fail(msg: &'static str) -> ! {
    panic!("{}", msg)
}
