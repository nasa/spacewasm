use crate::alloc::{AllocError, Allocator, GlobalAllocator};
use crate::util::InnerVec;
use core::alloc::Layout;
use core::ops::{Deref, DerefMut};

/// A fixed size vector allocated on the heap.
/// The capacity is set on construction and cannot be changed.
/// This is very similar to [::alloc::Vec] however it guarantees
/// maximum memory efficiency.
pub struct Vec<T: Sized, A: Allocator = GlobalAllocator> {
    inner: InnerVec<T>,
    alloc: A,
}

impl<T: Clone> Clone for Vec<T, GlobalAllocator> {
    fn clone(&self) -> Self {
        let mut n = Vec::new(self.inner.capacity).unwrap();
        if self.len() > 0 {
            n[0..self.len()].clone_from_slice(self);
            n.inner.len = self.inner.len;
        }

        n
    }
}

impl<T: Sized> Vec<T, GlobalAllocator> {
    pub fn new(capacity: u32) -> Result<Vec<T>, AllocError> {
        Vec::new_in(GlobalAllocator, capacity)
    }
}

impl<T: Sized, A: Allocator> Vec<T, A> {
    pub fn new_in(alloc: A, capacity: u32) -> Result<Vec<T, A>, AllocError> {
        // We don't want to handle ZST
        const {
            assert!(size_of::<T>() != 0);
        }

        let ptr = if capacity > 0 {
            unsafe { alloc.alloc(Layout::array::<T>(capacity as usize)?)? }
        } else {
            core::ptr::null_mut()
        };

        Ok(Vec {
            inner: InnerVec {
                ptr: ptr as *mut T,
                capacity,
                len: 0,
            },
            alloc,
        })
    }
}

impl<T: Sized> Vec<T, GlobalAllocator> {
    pub fn zero() -> Vec<T> {
        Vec {
            inner: InnerVec {
                ptr: core::ptr::null_mut(),
                capacity: 0,
                len: 0,
            },
            alloc: GlobalAllocator,
        }
    }
}

impl<T: Sized, A: Allocator> Vec<T, A> {
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Push a new item to the vector
    /// If the capacity is exceeded, this will panic
    pub fn push(&mut self, value: T) {
        self.inner.push(value)
    }

    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop()
    }

    pub fn iter(&self) -> impl Iterator<Item = T> {
        self.inner.iter()
    }
}

impl<T, A: Allocator> Deref for Vec<T, A> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.inner.deref()
    }
}

impl<T, A: Allocator> DerefMut for Vec<T, A> {
    fn deref_mut(&mut self) -> &mut [T] {
        self.inner.deref_mut()
    }
}

impl<T: Sized, A: Allocator> Drop for Vec<T, A> {
    fn drop(&mut self) {
        if self.inner.capacity != 0 {
            while let Some(_) = self.pop() {}
            unsafe {
                self.alloc.dealloc(
                    self.inner.ptr as *mut u8,
                    Layout::from_size_align(
                        size_of::<T>() * self.inner.capacity as usize,
                        align_of::<T>(),
                    )
                    .unwrap(),
                );
            }
        }
    }
}

pub struct IntoIter<T, A: Allocator = GlobalAllocator> {
    buf: *mut T,
    cap: usize,
    start: *const T,
    end: *const T,
    alloc: A,
}

impl<T, A: Allocator> Iterator for IntoIter<T, A> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                let result = core::ptr::read(self.start);
                self.start = self.start.offset(1);
                Some(result)
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = (self.end as usize - self.start as usize) / size_of::<T>();
        (len, Some(len))
    }
}

impl<T, A: Allocator> Drop for IntoIter<T, A> {
    fn drop(&mut self) {
        if self.cap != 0 {
            // drop any remaining elements
            for _ in &mut *self {}
            let layout = Layout::array::<T>(self.cap).unwrap();
            unsafe {
                self.alloc.dealloc(self.buf as *mut u8, layout);
            }
        }
    }
}
