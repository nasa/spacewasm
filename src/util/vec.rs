use crate::Box;
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

impl<T> Vec<T, GlobalAllocator> {
    pub fn from_exact_iter(iter: impl ExactSizeIterator<Item = T>) -> Self {
        let mut o = Self::new(iter.len() as u32).unwrap();
        for i in iter {
            o.push(i);
        }
        o
    }
}

#[macro_export]
macro_rules! vec {
    () => (
        $crate::Vec::zero()
    );
    ($elem:expr; $n:expr) => (
        $crate::Vec::from_elem($elem, $n)
    );
    ($($x:expr),+ $(,)?) => (
        // Using `write_box_via_move` produces a dramatic improvement in stack usage for unoptimized
        // programs using this code path to construct large Vecs. We can't use `write_via_move`
        // because this entire invocation has to remain a call chain without `let` bindings, or else
        // inference and temporary lifetimes change and things break (see `vec-macro-rvalue-scope`,
        // `vec-macro-coercions`, and `autoderef-vec-box-fn-36786` tests).
        //
        // `box_assume_init_into_vec_unsafe` isn't actually safe but the way we use it here is. We
        // can't use an unsafe block as that would also wrap `$x`.
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

impl<T: Clone, A: Allocator + Clone> Clone for Vec<T, A> {
    fn clone(&self) -> Self {
        let mut n: Vec<T, A> = Vec::new_in(self.alloc.clone(), self.inner.capacity).unwrap();

        if !self.is_empty() {
            // SAFETY: We need to write to uninitialized memory without creating a reference to it.
            // Use ptr::write to initialize each element.
            unsafe {
                for i in 0..self.len() {
                    core::ptr::write(n.inner.ptr.add(i), self[i].clone());
                }
            }
        }

        // Only set len after initializing the memory
        n.inner.len = self.inner.len;

        n
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
        let mut v = Vec::new(N as u32)?;
        for i in a {
            v.push(i);
        }

        Ok(v)
    }

    pub fn new(capacity: u32) -> Result<Vec<T>, AllocError> {
        Vec::new_in(GlobalAllocator, capacity)
    }

    pub fn new_from(ptr: *mut T, capacity: u32) -> Vec<T> {
        Vec {
            inner: InnerVec {
                ptr,
                capacity,
                len: 0,
            },
            alloc: GlobalAllocator,
        }
    }
}

impl<T: Sized, A: Allocator> Vec<T, A> {
    pub fn new_from_with_alloc(ptr: *mut T, capacity: u32, alloc: A) -> Vec<T, A> {
        Vec {
            inner: InnerVec {
                ptr,
                capacity,
                len: 0,
            },
            alloc,
        }
    }

    pub fn new_in(alloc: A, capacity: u32) -> Result<Vec<T, A>, AllocError> {
        // We don't want to handle ZST
        const {
            assert!(size_of::<T>() != 0);
        }

        let ptr = if capacity > 0 {
            unsafe { alloc.alloc(Layout::array::<T>(capacity as usize).unwrap())? }
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

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Appends an element to the back of a collection.
    ///
    /// # Panics
    ///
    /// Panics if the new length exceeds the capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut vec = vec![1, 2];
    /// vec.push(3);
    /// assert_eq!(vec, [1, 2, 3]);
    /// ```
    ///
    /// # Time complexity
    ///
    /// Takes *O*(1) time.
    pub fn push(&mut self, value: T) {
        self.inner.push(value)
    }

    /// Removes the last element from a vector and returns it, or [`None`] if it
    /// is empty.
    ///
    /// If you'd like to pop the first element, consider using
    /// [`VecDeque::pop_front`] instead.
    ///
    /// [`VecDeque::pop_front`]: crate::collections::VecDeque::pop_front
    ///
    /// # Examples
    ///
    /// ```
    /// let mut vec = vec![1, 2, 3];
    /// assert_eq!(vec.pop(), Some(3));
    /// assert_eq!(vec, [1, 2]);
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
            while self.pop().is_some() {}
            unsafe {
                self.alloc.dealloc(
                    self.inner.ptr as *mut u8,
                    Layout::array::<T>(self.inner.capacity as usize).unwrap(),
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
    use crate::alloc::{AllocError, Allocator};
    use core::alloc::Layout;

    /// Stub allocator for Kani verification
    /// Tracks allocation layout to verify Drop passes correct parameters to dealloc
    #[derive(Clone, Copy)]
    struct KaniStubAllocator;

    // Track allocated layout to verify dealloc receives matching parameters
    static mut ALLOC_PTR: *mut u8 = core::ptr::null_mut();
    static mut ALLOC_SIZE: usize = 0;
    static mut ALLOC_ALIGN: usize = 0;

    unsafe impl Allocator for KaniStubAllocator {
        unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
            if layout.size() == 0 {
                Ok(core::ptr::null_mut())
            } else {
                // Return a symbolic non-null pointer
                let addr: usize = kani::any();
                kani::assume(addr != 0); // Non-null
                kani::assume(addr % layout.align() == 0); // Properly aligned
                kani::assume(addr < usize::MAX - layout.size()); // No wraparound

                let ptr = addr as *mut u8;

                // Track allocation parameters for verification in dealloc
                unsafe {
                    ALLOC_PTR = ptr;
                    ALLOC_SIZE = layout.size();
                    ALLOC_ALIGN = layout.align();
                }

                Ok(ptr)
            }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // Verify Drop passes the same layout that was allocated
            let alloc_ptr = unsafe { core::ptr::read_volatile(&raw const ALLOC_PTR) };
            let alloc_size = unsafe { core::ptr::read_volatile(&raw const ALLOC_SIZE) };
            let alloc_align = unsafe { core::ptr::read_volatile(&raw const ALLOC_ALIGN) };

            assert_eq!(
                ptr, alloc_ptr,
                "Dealloc pointer must match allocated pointer"
            );
            assert_eq!(
                layout.size(),
                alloc_size,
                "Dealloc size must match allocated size"
            );
            assert_eq!(
                layout.align(),
                alloc_align,
                "Dealloc align must match allocated align"
            );
        }

        fn memory_statistics(&self) -> crate::MemoryStatistics {
            crate::MemoryStatistics {
                total_bytes: 0,
                pad_bytes: 0,
            }
        }
    }

    #[kani::proof]
    #[kani::unwind(3)]
    fn proof_alloc_dealloc_safety() {
        let capacity: u32 = kani::any();
        kani::assume(capacity <= 2);

        let vec: Vec<u32, _> = Vec::new_in(KaniStubAllocator, capacity).unwrap();

        assert_eq!(vec.capacity(), capacity as usize);
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
}
