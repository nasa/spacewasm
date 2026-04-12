use crate::alloc::{self, Allocator, GlobalAllocator};
use core::alloc::Layout;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

/// A fixed size vector allocated on the heap.
/// The capacity is set on construction and cannot be changed.
/// This is very similar to [::alloc::Vec] however it guarantees
/// maximum memory efficiency.
pub struct Vec<T: Sized, A: Allocator = GlobalAllocator> {
    ptr: *mut T,
    capacity: u32,
    len: u32,
    alloc: A,
}

impl<T: core::fmt::Debug, A: Allocator> core::fmt::Debug for Vec<T, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Clone> Clone for Vec<T, GlobalAllocator> {
    fn clone(&self) -> Self {
        let mut n = Vec::new(self.capacity).unwrap();
        if self.len() > 0 {
            n[0..self.len()].clone_from_slice(self);
            n.len = self.len;
        }

        n
    }
}

impl<T: Sized> Vec<T, GlobalAllocator> {
    pub fn new(capacity: u32) -> Result<Vec<T>, alloc::AllocError> {
        Vec::new_in(GlobalAllocator, capacity)
    }
}

impl<T: Sized, A: Allocator> Vec<T, A> {
    pub fn new_in(alloc: A, capacity: u32) -> Result<Vec<T, A>, alloc::AllocError> {
        // We don't want to handle ZST
        const {
            assert!(size_of::<T>() != 0);
        }

        let ptr = if capacity > 0 {
            unsafe {
                alloc.alloc(Layout::from_size_align(
                    size_of::<T>() * capacity as usize,
                    align_of::<T>(),
                )?)?
            }
        } else {
            core::ptr::null_mut()
        };

        Ok(Vec {
            ptr: ptr as *mut T,
            capacity,
            len: 0,
            alloc,
        })
    }
}

impl<T: Sized> Vec<T, GlobalAllocator> {
    pub fn zero() -> Vec<T> {
        Vec {
            ptr: core::ptr::null_mut(),
            capacity: 0,
            len: 0,
            alloc: GlobalAllocator,
        }
    }
}

impl<T: Sized, A: Allocator> Vec<T, A> {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }

    /// Push a new item to the vector
    /// If the capacity is exceeded, this will panic
    pub fn push(&mut self, value: T) {
        assert!(self.len < self.capacity);

        unsafe {
            core::ptr::write(self.ptr.add(self.len as usize), value);
        }

        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(core::ptr::read(self.ptr.add(self.len as usize))) }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = T> {
        unsafe { RawValIter::new(&self) }
    }
}

impl<T, A: Allocator> Deref for Vec<T, A> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        if self.ptr.is_null() {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr, self.len as usize) }
        }
    }
}

impl<T, A: Allocator> DerefMut for Vec<T, A> {
    fn deref_mut(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len as usize) }
        }
    }
}

impl<T: Sized, A: Allocator> Drop for Vec<T, A> {
    fn drop(&mut self) {
        if self.capacity != 0 {
            while let Some(_) = self.pop() {}
            unsafe {
                self.alloc.dealloc(
                    self.ptr as *mut u8,
                    Layout::from_size_align(
                        size_of::<T>() * self.capacity as usize,
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

impl<T> IntoIterator for Vec<T, GlobalAllocator> {
    type Item = T;
    type IntoIter = IntoIter<T, GlobalAllocator>;
    fn into_iter(self) -> IntoIter<T, GlobalAllocator> {
        // Make sure not to drop Vec since that would free the buffer
        let vec = core::mem::ManuallyDrop::new(self);

        // Can't destructure Vec since it's Drop
        let ptr = vec.ptr;
        let cap = vec.capacity as usize;
        let len = vec.len as usize;

        IntoIter {
            buf: ptr,
            cap,
            start: ptr,
            end: if cap == 0 {
                // can't offset off this pointer, it's not allocated!
                ptr
            } else {
                unsafe { ptr.add(len) }
            },
            alloc: GlobalAllocator,
        }
    }
}

struct RawValIter<T> {
    start: *const T,
    end: *const T,
}

impl<T> RawValIter<T> {
    unsafe fn new(slice: &[T]) -> Self {
        unsafe {
            RawValIter {
                start: slice.as_ptr(),
                end: if size_of::<T>() == 0 {
                    ((slice.as_ptr() as usize) + slice.len()) as *const _
                } else if slice.len() == 0 {
                    slice.as_ptr()
                } else {
                    slice.as_ptr().add(slice.len())
                },
            }
        }
    }
}

impl<T> Iterator for RawValIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                if size_of::<T>() == 0 {
                    self.start = (self.start as usize + 1) as *const _;
                    Some(core::ptr::read(NonNull::<T>::dangling().as_ptr()))
                } else {
                    let old_ptr = self.start;
                    self.start = self.start.offset(1);
                    Some(core::ptr::read(old_ptr))
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let elem_size = size_of::<T>();
        let len =
            (self.end as usize - self.start as usize) / if elem_size == 0 { 1 } else { elem_size };
        (len, Some(len))
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<T> {
        if self.start == self.end {
            None
        } else {
            unsafe {
                self.end = self.end.offset(-1);
                Some(core::ptr::read(self.end))
            }
        }
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
