use crate::alloc::{AllocError, Allocator, GlobalAllocator};
use crate::util::InnerVec;
use crate::Box;
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

impl<A: Allocator, T: core::fmt::Debug> core::fmt::Debug for Vec<T, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: Clone, A: Allocator + Clone> Clone for Vec<T, A> {
    fn clone(&self) -> Self {
        let mut n = Vec::new_in(self.alloc.clone(), self.inner.capacity).unwrap();
        n.inner.len = self.inner.len;
        n.inner.capacity = self.inner.capacity;

        if self.len() > 0 {
            n[0..self.len()].clone_from_slice(self);
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

    pub unsafe fn assume_init(mut self) -> Self {
        self.inner.len = self.inner.capacity;
        self
    }

    pub fn into_boxed_slice(self) -> Box<[T], A> {
        assert_eq!(self.capacity(), self.len());

        unsafe {
            let ptr = self.inner.ptr;
            let cap = self.inner.capacity;
            let alloc = core::ptr::read(&self.alloc);

            core::mem::forget(self);

            let slice_ptr: *mut [T] = core::ptr::slice_from_raw_parts_mut(ptr, cap as usize);

            Box::from_raw(alloc, slice_ptr)
        }
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

impl<T> IntoIterator for Vec<T, GlobalAllocator> {
    type Item = T;
    type IntoIter = IntoIter<T, GlobalAllocator>;
    fn into_iter(self) -> IntoIter<T, GlobalAllocator> {
        // Make sure not to drop Vec since that would free the buffer
        let vec = core::mem::ManuallyDrop::new(self);

        // Can't destructure Vec since it's Drop
        let ptr = vec.inner.ptr;
        let cap = vec.inner.capacity as usize;
        let len = vec.inner.len as usize;

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

#[kani::proof]
fn proof_zero() {
    let vec: Vec<i32> = Vec::zero();
    assert_eq!(vec.len(), 0);
    assert_eq!(vec.capacity(), 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero() {
        let vec: Vec<i32> = Vec::zero();
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.capacity(), 0);
    }

    #[test]
    fn test_push_pop() {
        let mut vec = Vec::new(5).unwrap();

        vec.push(10);
        vec.push(20);
        vec.push(30);
        assert_eq!(vec.len(), 3);

        assert_eq!(vec.pop(), Some(30));
        assert_eq!(vec.pop(), Some(20));
        assert_eq!(vec.pop(), Some(10));
        assert_eq!(vec.pop(), None);
    }

    #[test]
    #[should_panic]
    fn test_push_exceeds_capacity() {
        let mut vec = Vec::new(2).unwrap();
        vec.push(1);
        vec.push(2);
        vec.push(3);
    }

    #[test]
    fn test_deref() {
        let mut vec = Vec::new(3).unwrap();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        let slice: &[i32] = &*vec;
        assert_eq!(slice, &[1, 2, 3]);
    }

    #[test]
    fn test_deref_mut() {
        let mut vec = Vec::new(3).unwrap();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        vec[0] = 10;
        assert_eq!(vec[0], 10);
    }

    #[test]
    fn test_clone() {
        let mut vec = Vec::new(3).unwrap();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        let cloned = vec.clone();
        assert_eq!(vec.len(), cloned.len());
        assert_eq!(&vec[..], &cloned[..]);
    }
}
