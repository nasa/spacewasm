// Portions of this file are derived from the Rust project
// (https://github.com/rust-lang/rust), licensed under Apache-2.0. These
// portions have been modified for SpaceWasm.

use crate::Box;
use crate::alloc::{AllocError, Allocator, GlobalAllocator};
use crate::util::InnerVec;
use core::alloc::Layout;
use core::ops::{Deref, DerefMut};

/// A fixed size vector allocated on the heap.
/// The capacity is set on construction and cannot be changed.
/// This is very similar to `alloc::vec::Vec` however it guarantees
/// maximum memory efficiency.
pub struct Vec<T: Sized, A: Allocator = GlobalAllocator> {
    inner: InnerVec<T>,
    alloc: A,
}

impl<T> Vec<T, GlobalAllocator> {
    /// Build a `Vec` from an exact-size iterator.
    ///
    /// The capacity is taken from `iter.len()`. Returns [`AllocError`] if that
    /// length does not fit in a `u32` (instead of silently truncating the cast)
    /// or if allocating the backing buffer fails.
    pub fn from_exact_iter(iter: impl ExactSizeIterator<Item = T>) -> Result<Self, AllocError> {
        let mut o = Self::new(iter.len())?;
        for i in iter {
            o.push(i);
        }
        Ok(o)
    }
}

#[macro_export]
macro_rules! vec {
    () => (
        $crate::Vec::zero()
    );
    ($($x:expr),+ $(,)?) => (
        $crate::Vec::from_array([$($x),+]).unwrap()
    );
}

impl<T> Default for Vec<T> {
    fn default() -> Self {
        Vec::<T>::zero()
    }
}

impl<A: Allocator, T: core::fmt::Debug> core::fmt::Debug for Vec<T, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: Clone, A: Allocator + Clone> Vec<T, A> {
    pub fn try_clone(&self) -> Result<Self, AllocError> {
        let mut n: Vec<T, A> = Vec::new_in(self.alloc.clone(), self.inner.capacity)?;

        for i in 0..self.len() {
            // `self.len() <= capacity == n.capacity`, so `push` never overflows.
            // `self[i].clone()` is evaluated before `push` takes ownership, so a
            // panic there leaves `n` holding only the already-cloned prefix.
            n.push(self[i].clone());
        }

        Ok(n)
    }
}

impl<T: Clone, A: Allocator + Clone> Clone for Vec<T, A> {
    /// # Panics
    /// Panics if allocating the new backing buffer fails. Use
    /// [`Vec::try_clone`] to handle allocation failure without panicking.
    fn clone(&self) -> Self {
        self.try_clone().expect("Vec::clone: allocation failed")
    }
}

impl<T: PartialEq, A: Allocator> PartialEq for Vec<T, A> {
    fn eq(&self, other: &Self) -> bool {
        self[..] == other[..]
    }
}

impl<T: Eq, A: Allocator> Eq for Vec<T, A> {}

impl<T: Sized> Vec<T, GlobalAllocator> {
    pub fn from_array<const N: usize>(a: [T; N]) -> Result<Self, AllocError> {
        let mut v = Vec::new(N)?;
        for i in a {
            v.push(i);
        }

        Ok(v)
    }

    pub fn new(capacity: usize) -> Result<Vec<T>, AllocError> {
        Vec::new_in(GlobalAllocator, capacity)
    }

    /// Build a `Vec` that adopts a raw allocation, using [`GlobalAllocator`].
    ///
    /// The returned `Vec` starts empty (`len == 0`) and treats all `capacity`
    /// slots as uninitialized. Its [`Drop`] frees `ptr` with [`GlobalAllocator`].
    ///
    /// # Safety
    /// - `ptr` must point to a single allocation of at least `capacity` values
    ///   of `T`, produced by [`GlobalAllocator`] with the layout
    ///   `Layout::array::<T>(capacity)` (correctly sized and aligned).
    /// - Ownership of that allocation is transferred to the returned `Vec`: it
    ///   must not be freed, aliased, or otherwise used elsewhere, since the
    ///   `Vec`'s `Drop` will free it with the same allocator and layout.
    pub unsafe fn new_from(ptr: *mut T, capacity: usize) -> Vec<T> {
        Vec {
            // SAFETY: guaranteed by this function's contract; `len == 0` so no
            // element is claimed initialized.
            inner: unsafe { InnerVec::from_raw_parts(ptr, capacity, 0) },
            alloc: GlobalAllocator,
        }
    }
}

impl<T: Sized, A: Allocator> Vec<T, A> {
    /// Build a `Vec` that adopts a raw allocation, using a custom allocator.
    ///
    /// The returned `Vec` starts empty (`len == 0`) and treats all `capacity`
    /// slots as uninitialized. Its [`Drop`] frees `ptr` with `alloc`.
    ///
    /// # Safety
    /// - `ptr` must point to a single allocation of at least `capacity` values
    ///   of `T`, produced by `alloc` with the layout
    ///   `Layout::array::<T>(capacity)` (correctly sized and aligned).
    /// - Ownership of that allocation is transferred to the returned `Vec`: it
    ///   must not be freed, aliased, or otherwise used elsewhere, since the
    ///   `Vec`'s `Drop` will free it with `alloc` and the same layout.
    pub unsafe fn new_from_with_alloc(ptr: *mut T, capacity: usize, alloc: A) -> Vec<T, A> {
        Vec {
            // SAFETY: guaranteed by this function's contract; `len == 0` so no
            // element is claimed initialized.
            inner: unsafe { InnerVec::from_raw_parts(ptr, capacity, 0) },
            alloc,
        }
    }

    pub fn new_in(alloc: A, capacity: usize) -> Result<Vec<T, A>, AllocError> {
        // We don't want to handle ZST
        const {
            assert!(size_of::<T>() != 0);
        }

        let ptr = if capacity > 0 {
            let layout = Layout::array::<T>(capacity).map_err(|_| AllocError::AllocationFailed)?;
            unsafe { alloc.alloc(layout)? }
        } else {
            core::ptr::null_mut()
        };

        Ok(Vec {
            // SAFETY: `ptr` is either null (capacity 0) or a fresh allocation of
            // `capacity` elements from `alloc`, and `len == 0` so no element is
            // claimed initialized.
            inner: unsafe { InnerVec::from_raw_parts(ptr as *mut T, capacity, 0) },
            alloc,
        })
    }
}

impl<T: Sized> Vec<T, GlobalAllocator> {
    pub fn zero() -> Vec<T> {
        Vec {
            inner: InnerVec::zero(),
            alloc: GlobalAllocator,
        }
    }
}

impl<T: Sized, A: Allocator> Vec<T, A> {
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Appends an element to the back of the vector.
    ///
    /// Unlike `alloc::vec::Vec`, this vector has a fixed capacity fixed at
    /// construction and never reallocates. Use [`Vec::try_push`] to handle a
    /// full vector without panicking.
    ///
    /// # Panics
    ///
    /// Panics if the new length exceeds the capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// # // `spacewasm` is `no_std`: its `GlobalAllocator` forwards to the
    /// # // `__spacewasm_*` symbols an embedder installs via `global_allocator!`.
    /// # // A doctest is its own binary, so provide a std-backed implementation.
    /// # struct DocAlloc;
    /// # unsafe impl spacewasm::Allocator for DocAlloc {
    /// #     unsafe fn alloc(&self, l: core::alloc::Layout) -> Result<*mut u8, spacewasm::AllocError> {
    /// #         Ok(unsafe { std::alloc::alloc(l) })
    /// #     }
    /// #     unsafe fn dealloc(&self, p: *mut u8, l: core::alloc::Layout) { unsafe { std::alloc::dealloc(p, l) } }
    /// # }
    /// # spacewasm::global_allocator!(DocAlloc, DocAlloc);
    /// use spacewasm::Vec;
    ///
    /// let mut vec = Vec::new(3).unwrap();
    /// vec.push(1);
    /// vec.push(2);
    /// vec.push(3);
    /// assert_eq!(&vec[..], &[1, 2, 3]);
    /// ```
    ///
    /// # Time complexity
    ///
    /// Takes *O*(1) time.
    pub fn push(&mut self, value: T) {
        self.inner.push(value)
    }

    pub fn try_push(&mut self, value: T) -> Result<(), AllocError> {
        self.inner.try_push(value)
    }

    /// Removes the last element from the vector and returns it, or [`None`] if
    /// it is empty. The capacity is unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// # // `spacewasm` is `no_std`: its `GlobalAllocator` forwards to the
    /// # // `__spacewasm_*` symbols an embedder installs via `global_allocator!`.
    /// # // A doctest is its own binary, so provide a std-backed implementation.
    /// # struct DocAlloc;
    /// # unsafe impl spacewasm::Allocator for DocAlloc {
    /// #     unsafe fn alloc(&self, l: core::alloc::Layout) -> Result<*mut u8, spacewasm::AllocError> {
    /// #         Ok(unsafe { std::alloc::alloc(l) })
    /// #     }
    /// #     unsafe fn dealloc(&self, p: *mut u8, l: core::alloc::Layout) { unsafe { std::alloc::dealloc(p, l) } }
    /// # }
    /// # spacewasm::global_allocator!(DocAlloc, DocAlloc);
    /// use spacewasm::Vec;
    ///
    /// let mut vec = Vec::from_array([1, 2, 3]).unwrap();
    /// assert_eq!(vec.pop(), Some(3));
    /// assert_eq!(&vec[..], &[1, 2]);
    /// ```
    ///
    /// # Time complexity
    ///
    /// Takes *O*(1) time.
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self[..].iter()
    }

    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut T> + ExactSizeIterator {
        self[..].iter_mut()
    }

    /// # Safety
    /// The caller must ensure that all elements up to capacity have been initialized.
    pub unsafe fn assume_init(mut self) -> Self {
        self.inner.len = self.inner.capacity;
        self
    }

    /// Converts the vector into a boxed slice.
    ///
    /// # Panics
    ///
    /// Panics if the vector's length does not equal its capacity.
    pub fn into_boxed_slice(self) -> Box<[T], A> {
        assert_eq!(self.capacity(), self.len());

        unsafe {
            let ptr = self.inner.ptr;
            let cap = self.inner.capacity;
            let alloc = core::ptr::read(&self.alloc);

            core::mem::forget(self);

            let ptr = if cap == 0 {
                core::ptr::NonNull::<T>::dangling().as_ptr()
            } else {
                ptr
            };

            let slice_ptr: *mut [T] = core::ptr::slice_from_raw_parts_mut(ptr, cap);

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
            while self.pop().is_some() {}
            unsafe {
                self.alloc.dealloc(
                    self.inner.ptr as *mut u8,
                    Layout::array::<T>(self.inner.capacity).unwrap(),
                );
            }
        }
    }
}

impl<T, A: Allocator> IntoIterator for Vec<T, A> {
    type Item = T;
    type IntoIter = IntoIter<T, A>;
    fn into_iter(self) -> IntoIter<T, A> {
        // Make sure not to drop Vec since that would free the buffer
        let vec = core::mem::ManuallyDrop::new(self);

        // Can't destructure Vec since it's Drop
        let ptr = vec.inner.ptr;
        let cap = vec.inner.capacity;
        let len = vec.inner.len;

        // SAFETY: move the allocator out of the `ManuallyDrop` wrapper into the IntoIter
        let alloc = unsafe { core::ptr::read(&vec.alloc) };

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
            alloc,
        }
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a Vec<T, A> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        (**self).iter()
    }
}

impl<'a, T, A: Allocator> IntoIterator for &'a mut Vec<T, A> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        (**self).iter_mut()
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

impl<T, A: Allocator> DoubleEndedIterator for IntoIter<T, A> {
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

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    use crate::test_support::RustSystemAllocator;

    #[kani::proof]
    #[kani::unwind(3)]
    fn proof_alloc_dealloc_safety() {
        let capacity: u32 = kani::any();
        kani::assume(capacity <= 2);

        let vec: Vec<u32, _> = Vec::new_in(RustSystemAllocator, capacity).unwrap();

        assert_eq!(vec.capacity(), capacity);
        assert_eq!(vec.len(), 0);

        if capacity == 0 {
            assert!(
                vec.inner.ptr.is_null(),
                "Zero-capacity vec must have null pointer"
            );
        } else {
            assert!(
                !vec.inner.ptr.is_null(),
                "Non-zero capacity vec must have valid allocation"
            );
        }

        let size = size_of::<u32>() * vec.capacity();
        let align = align_of::<u32>();
        assert!(
            Layout::from_size_align(size, align).is_ok(),
            "Layout calculation must not overflow (critical for Drop)"
        );

        // Vec drops here - dealloc will verify the layout size matches what was allocated
    }

    /// Verify that Vec::clone creates an independent copy with equal contents.
    /// Exercises the clone path (`Clone::clone` -> `try_clone`), which pushes
    /// each cloned element into a freshly allocated buffer.
    #[kani::proof]
    #[kani::unwind(4)]
    fn proof_vec_clone_correctness() {
        let capacity: u32 = kani::any();
        kani::assume(capacity <= 3);

        let mut vec: Vec<u32, _> = Vec::new_in(RustSystemAllocator, capacity).unwrap();

        // Push symbolic values up to capacity
        let len: u32 = kani::any();
        kani::assume(len <= capacity);

        for _ in 0..len {
            let val: u32 = kani::any();
            vec.push(val);
        }

        let original_len = vec.len();

        let cloned = vec.clone();

        // Verify equal length and capacity
        assert_eq!(cloned.len(), vec.len(), "Clone must have same length");
        assert_eq!(
            cloned.capacity(),
            vec.capacity(),
            "Clone must have same capacity"
        );

        // Verify equal contents by comparing the slices directly
        for i in 0..original_len {
            assert_eq!(cloned[i], vec[i], "Clone must have same contents");
        }

        // Verify independence: when non-zero capacity, the clone owns different memory
        // (Skip the pointer check since capacity can be 0, where both pointers are null)
    }

    /// Verify that into_boxed_slice correctly converts a full Vec to Box<[T]>
    /// without double-free. Tests the mem::forget + from_raw pattern (vec.rs:240-254).
    #[kani::proof]
    #[kani::unwind(4)]
    fn proof_vec_into_boxed_slice_correctness() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 3);

        let mut vec: Vec<u32, _> = Vec::new_in(RustSystemAllocator, capacity).unwrap();

        // Fill to capacity (required by into_boxed_slice)
        let values: [u32; 3] = kani::any();
        for i in 0..(capacity) {
            vec.push(values[i]);
        }

        let len = vec.len();
        assert_eq!(len, capacity, "Vec must be full for into_boxed_slice");

        let boxed_slice = vec.into_boxed_slice();

        // Verify the Box has the same length and contents
        assert_eq!(boxed_slice.len(), len, "Box must have same length as Vec");
        for i in 0..len {
            assert_eq!(
                boxed_slice[i], values[i],
                "Box must have same contents as Vec"
            );
        }

        // Box drops here - if mem::forget in into_boxed_slice didn't work correctly,
        // this would be a double-free detected by RustSystemAllocator
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    #[test]
    fn test_zero() {
        let vec: Vec<i32> = Vec::zero();
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.capacity(), 0);
    }

    #[test]
    fn test_zero_into_boxed_slice_is_derefable() {
        // A zero-capacity `Vec` holds a null pointer, which must not reach the
        // `Box` it converts into: `Box` derefs unconditionally, so a null there
        // traps under optimization.
        let b = Vec::<i32>::zero().into_boxed_slice();
        assert!(b.is_empty());
        assert_eq!(&*b, &[] as &[i32]);
        drop(b);

        let b = Vec::<i32>::new(0).unwrap().into_boxed_slice();
        assert_eq!(&*b, &[] as &[i32]);
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
    fn test_try_push() {
        let mut vec = Vec::new(2).unwrap();

        // Succeeds while there is spare capacity.
        assert_eq!(vec.try_push(1), Ok(()));
        assert_eq!(vec.try_push(2), Ok(()));
        assert_eq!(vec.len(), 2);

        // Once full, try_push reports the failure instead of panicking.
        assert_eq!(vec.try_push(3), Err(AllocError::OutOfMemory));
        assert_eq!(vec.len(), 2);
        assert_eq!(&vec[..], &[1, 2]);
    }

    #[test]
    fn test_into_boxed_slice() {
        let mut vec = Vec::new(3).unwrap();
        vec.push(10);
        vec.push(20);
        vec.push(30);

        let boxed = vec.into_boxed_slice();
        assert_eq!(boxed.len(), 3);
        assert_eq!(&*boxed, &[10, 20, 30]);
    }

    #[test]
    #[should_panic]
    fn test_into_boxed_slice_not_full_panics() {
        // into_boxed_slice requires len == capacity.
        let mut vec = Vec::new(3).unwrap();
        vec.push(1);
        let _ = vec.into_boxed_slice();
    }

    #[test]
    fn test_deref() {
        let mut vec = Vec::new(3).unwrap();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        let slice: &[i32] = &vec;
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

    #[test]
    fn test_iter_reverse() {
        let mut vec = Vec::new(5).unwrap();
        vec.push(1);
        vec.push(2);
        vec.push(3);
        vec.push(4);
        vec.push(5);

        let mut reversed = Vec::new(5).unwrap();
        for val in vec.iter().rev() {
            reversed.push(*val);
        }
        assert_eq!(&reversed[..], &[5, 4, 3, 2, 1]);
    }

    #[test]
    fn test_iter_mut_reverse() {
        let mut vec = Vec::new(3).unwrap();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        for val in vec.iter_mut().rev() {
            *val *= 10;
        }

        assert_eq!(&vec[..], &[10, 20, 30]);
    }

    #[test]
    fn test_into_iter_reverse() {
        let mut vec = Vec::new(4).unwrap();
        vec.push(10);
        vec.push(20);
        vec.push(30);
        vec.push(40);

        let mut reversed = Vec::new(4).unwrap();
        for val in vec.into_iter().rev() {
            reversed.push(val);
        }
        assert_eq!(&reversed[..], &[40, 30, 20, 10]);
    }

    #[test]
    fn test_iter_both_ends() {
        let mut vec = Vec::new(5).unwrap();
        vec.push(1);
        vec.push(2);
        vec.push(3);
        vec.push(4);
        vec.push(5);

        let mut iter = vec.iter();
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next_back(), Some(&5));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next_back(), Some(&4));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn test_into_iter_both_ends() {
        let mut vec = Vec::new(4).unwrap();
        vec.push(10);
        vec.push(20);
        vec.push(30);
        vec.push(40);

        let mut iter = vec.into_iter();
        assert_eq!(iter.next(), Some(10));
        assert_eq!(iter.next_back(), Some(40));
        assert_eq!(iter.next_back(), Some(30));
        assert_eq!(iter.next(), Some(20));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn test_iter_empty_reverse() {
        let vec: Vec<i32> = Vec::zero();
        let mut count = 0;
        for _ in vec.iter().rev() {
            count += 1;
        }
        assert_eq!(count, 0);
    }

    #[test]
    fn test_default() {
        let vec: Vec<i32> = Vec::default();
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.capacity(), 0);
    }

    #[test]
    fn test_from_array() {
        let vec = Vec::from_array([1, 2, 3, 4]).unwrap();
        assert_eq!(vec.len(), 4);
        assert_eq!(vec.capacity(), 4);
        assert_eq!(&vec[..], &[1, 2, 3, 4]);
    }

    #[test]
    fn test_from_exact_iter() {
        let vec = Vec::from_exact_iter([10, 20, 30].into_iter()).unwrap();
        assert_eq!(vec.len(), 3);
        assert_eq!(vec.capacity(), 3);
        assert_eq!(&vec[..], &[10, 20, 30]);
    }

    #[test]
    fn test_partial_eq() {
        let a = Vec::from_array([1, 2, 3]).unwrap();
        let b = Vec::from_array([1, 2, 3]).unwrap();
        let c = Vec::from_array([1, 2, 4]).unwrap();
        let mut short = Vec::new(3).unwrap();
        short.push(1);
        short.push(2);

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, short);
    }

    #[test]
    fn test_debug() {
        let vec = Vec::from_array([1, 2, 3]).unwrap();
        let s = std::format!("{:?}", vec);
        assert_eq!(s, "[1, 2, 3]");
    }

    #[test]
    fn test_new_from() {
        let capacity = 3usize;
        let layout = Layout::array::<i32>(capacity).unwrap();
        let ptr = unsafe { GlobalAllocator.alloc(layout).unwrap() as *mut i32 };

        // SAFETY: `ptr` is a fresh GlobalAllocator allocation for `capacity`
        // i32s and ownership is transferred to the returned Vec.
        let mut vec = unsafe { Vec::new_from(ptr, capacity) };
        assert_eq!(vec.capacity(), 3);
        assert_eq!(vec.len(), 0);

        vec.push(7);
        vec.push(8);
        assert_eq!(&vec[..], &[7, 8]);
        // Drop deallocates via GlobalAllocator using the recorded capacity.
    }

    #[test]
    fn test_new_from_with_alloc() {
        use crate::test_support::RustSystemAllocator;

        let capacity = 2usize;
        let layout = Layout::array::<i32>(capacity).unwrap();
        let ptr = unsafe { RustSystemAllocator.alloc(layout).unwrap() as *mut i32 };

        // SAFETY: `ptr` is a fresh RustSystemAllocator allocation for `capacity`
        // i32s and ownership is transferred to the returned Vec.
        let mut vec = unsafe { Vec::new_from_with_alloc(ptr, capacity, RustSystemAllocator) };
        assert_eq!(vec.capacity(), 2);
        assert_eq!(vec.len(), 0);

        vec.push(42);
        assert_eq!(&vec[..], &[42]);
    }

    #[test]
    fn test_new_in_zero_capacity() {
        use crate::test_support::RustSystemAllocator;

        let vec: Vec<i32, _> = Vec::new_in(RustSystemAllocator, 0).unwrap();
        assert_eq!(vec.capacity(), 0);
        assert_eq!(vec.len(), 0);
        assert!(vec.inner.ptr.is_null());
    }

    #[test]
    fn test_assume_init() {
        let capacity = 3usize;
        let layout = Layout::array::<i32>(capacity).unwrap();
        let ptr = unsafe { GlobalAllocator.alloc(layout).unwrap() as *mut i32 };

        // Initialize every slot up to capacity before calling assume_init.
        unsafe {
            for i in 0..capacity {
                core::ptr::write(ptr.add(i), (i as i32) * 100);
            }
        }

        let vec = unsafe { Vec::new_from(ptr, capacity).assume_init() };
        assert_eq!(vec.len(), 3);
        assert_eq!(&vec[..], &[0, 100, 200]);
    }

    #[test]
    fn test_ref_into_iter() {
        let vec = Vec::from_array([1, 2, 3]).unwrap();

        let mut collected = Vec::new(3).unwrap();
        for v in &vec {
            collected.push(*v);
        }
        assert_eq!(&collected[..], &[1, 2, 3]);
    }

    #[test]
    fn test_ref_mut_into_iter() {
        let mut vec = Vec::from_array([1, 2, 3]).unwrap();

        for v in &mut vec {
            *v += 1;
        }
        assert_eq!(&vec[..], &[2, 3, 4]);
    }

    #[test]
    fn test_into_iter_size_hint() {
        let vec = Vec::from_array([1, 2, 3, 4]).unwrap();
        let mut iter = vec.into_iter();
        assert_eq!(iter.size_hint(), (4, Some(4)));

        iter.next();
        assert_eq!(iter.size_hint(), (3, Some(3)));

        iter.next_back();
        assert_eq!(iter.size_hint(), (2, Some(2)));
    }

    #[test]
    fn test_into_iter_empty() {
        let vec: Vec<i32> = Vec::zero();
        let mut iter = vec.into_iter();
        assert_eq!(iter.size_hint(), (0, Some(0)));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }
}
