#![no_std]

pub mod decode;
pub use decode::*;

pub mod util;
pub use util::*;

pub mod common;
pub use common::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global_allocator;
    use crate::StaticAllocator;

    global_allocator!(StaticAllocator<1024, 8>, StaticAllocator::new());
}
