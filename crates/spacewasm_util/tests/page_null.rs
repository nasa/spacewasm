//! A page allocation that fails must be reported as an error, not handed back as
//! a successful allocation based at address zero.
//!
//! This rests on the real system allocator returning NULL, which Miri does not
//! model, so it is excluded there like the other integration tests.
#![cfg(not(miri))]

use spacewasm::{Allocator, PageAllocator};
use spacewasm_util::RustSystemAllocator;
use std::alloc::Layout;

/// Under `isize::MAX` so `Layout` accepts it, but far beyond the address space of
/// either pointer width, so the request cannot be satisfied and the system
/// allocator returns NULL.
const UNSATISFIABLE_PAGE: usize = 1usize << (usize::BITS - 2);

#[test]
fn failed_page_allocation_is_reported_as_an_error() {
    let page_layout = Layout::from_size_align(UNSATISFIABLE_PAGE, 8).unwrap();

    // Guard against a platform that somehow satisfies the request: without a real
    // failure there is nothing to observe, so leave rather than assert.
    let probe = unsafe { std::alloc::alloc(page_layout) };
    if !probe.is_null() {
        unsafe { std::alloc::dealloc(probe, page_layout) };
        eprintln!("skipped: the system satisfied a {UNSATISFIABLE_PAGE:#x} byte request");
        return;
    }

    let page_alloc: PageAllocator<RustSystemAllocator, 4> =
        PageAllocator::new(RustSystemAllocator, UNSATISFIABLE_PAGE);

    if let Ok(ptr) = unsafe { page_alloc.alloc(Layout::from_size_align(16, 8).unwrap()) } {
        panic!("reported success with {ptr:p} after the page could not be allocated");
    }
}
