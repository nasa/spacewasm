use crate::alloc::AllocError;
use crate::ValidationError;
use core::ops::{Deref, DerefMut};

pub struct StaticVec<T: Sized, const N: usize> {
    data: [T; N],
    len: u32,
}

impl<T: Sized, const N: usize> StaticVec<T, N> {
    pub(crate) fn truncate(&mut self, new_len: usize) {
        assert!(new_len <= self.len as usize);
        self.len = new_len as u32;
    }
}

impl<T: Sized, const N: usize> Default for StaticVec<T, N> {
    fn default() -> Self {
        Self {
            data: unsafe { core::mem::zeroed() },
            len: 0,
        }
    }
}

impl<'a, const N: usize> TryInto<&'a str> for &'a StaticVec<u8, N> {
    type Error = ValidationError;

    fn try_into(self) -> Result<&'a str, Self::Error> {
        core::str::from_utf8(&self[0..(self.len as usize)])
            .map_err(|_| ValidationError::MalformedUtf8)
    }
}

impl<T, const N: usize> StaticVec<T, N> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, value: T) -> Result<(), AllocError> {
        if (self.len as usize) >= N {
            return Err(AllocError::OutOfMemory);
        }

        unsafe {
            core::ptr::write(&mut self.data[self.len as usize], value);
        }

        self.len += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(core::ptr::read(&self.data[self.len as usize])) }
        }
    }
}

impl<T: core::fmt::Debug, const N: usize> core::fmt::Debug for StaticVec<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T, const N: usize> Deref for StaticVec<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.data[0..(self.len as usize)]
    }
}

impl<T, const N: usize> DerefMut for StaticVec<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data[0..(self.len as usize)]
    }
}

pub struct StaticVecIntoIter<T, const N: usize> {
    vec: core::mem::ManuallyDrop<StaticVec<T, N>>,
    pos: usize,
}

impl<T, const N: usize> Iterator for StaticVecIntoIter<T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < (self.vec.len as usize) {
            let item = unsafe { core::ptr::read(&self.vec.data[self.pos]) };
            self.pos += 1;
            Some(item)
        } else {
            None
        }
    }
}

impl<T, const N: usize> Drop for StaticVecIntoIter<T, N> {
    fn drop(&mut self) {
        // Drop remaining elements that haven't been yielded yet
        while self.pos < (self.vec.len as usize) {
            unsafe { core::ptr::drop_in_place(&mut self.vec.data[self.pos] as *mut T) };
            self.pos += 1;
        }
    }
}

impl<T, const N: usize> IntoIterator for StaticVec<T, N> {
    type Item = T;
    type IntoIter = StaticVecIntoIter<T, N>;

    fn into_iter(self) -> Self::IntoIter {
        StaticVecIntoIter {
            vec: core::mem::ManuallyDrop::new(self),
            pos: 0,
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Verify StaticVec push and pop operations maintain LIFO ordering, length invariants,
    /// correctly enforce capacity limits, and deref returns correct slice view.
    #[kani::proof]
    #[kani::unwind(6)]
    fn proof_push_pop_correctness() {
        let mut vec: StaticVec<u32, 3> = StaticVec::new();

        assert_eq!(vec.len(), 0, "new vector should be empty");

        // Generate arbitrary values to push
        let v1: u32 = kani::any();
        let v2: u32 = kani::any();
        let v3: u32 = kani::any();
        let v4: u32 = kani::any();

        // Push values up to capacity and verify length increases
        assert!(vec.push(v1).is_ok(), "first push should succeed");
        assert_eq!(vec.len(), 1, "length should be 1 after first push");

        assert!(vec.push(v2).is_ok(), "second push should succeed");
        assert_eq!(vec.len(), 2, "length should be 2 after second push");

        // Verify deref returns correct slice view
        let slice: &[u32] = &*vec;
        assert_eq!(slice.len(), 2, "deref slice length should match vector length");
        assert_eq!(slice[0], v1, "first element should match first pushed value");
        assert_eq!(slice[1], v2, "second element should match second pushed value");

        assert!(vec.push(v3).is_ok(), "third push should succeed");
        assert_eq!(vec.len(), 3, "length should be 3 after third push");

        // Test capacity limit: push beyond capacity should fail
        let result = vec.push(v4);
        assert!(result.is_err(), "push beyond capacity should fail");
        assert!(
            matches!(result, Err(AllocError::OutOfMemory)),
            "push beyond capacity should return OutOfMemory"
        );
        assert_eq!(vec.len(), 3, "length should remain at capacity after failed push");

        // Pop values and verify LIFO ordering
        assert_eq!(vec.pop(), Some(v3), "should pop third value first");
        assert_eq!(vec.len(), 2, "length should be 2 after first pop");

        assert_eq!(vec.pop(), Some(v2), "should pop second value");
        assert_eq!(vec.len(), 1, "length should be 1 after second pop");

        assert_eq!(vec.pop(), Some(v1), "should pop first value last");
        assert_eq!(vec.len(), 0, "vector should be empty after popping all elements");

        assert_eq!(vec.pop(), None, "popping empty vector should return None");
    }

    /// Verify StaticVec IntoIterator yields values in correct order and drops properly.
    #[kani::proof]
    #[kani::unwind(5)]
    fn proof_into_iter_correctness() {
        let mut vec: StaticVec<u32, 4> = StaticVec::new();

        let v1: u32 = kani::any();
        let v2: u32 = kani::any();
        let v3: u32 = kani::any();

        vec.push(v1).unwrap();
        vec.push(v2).unwrap();
        vec.push(v3).unwrap();

        let mut iter = vec.into_iter();

        assert_eq!(iter.next(), Some(v1), "first next should yield first value");
        assert_eq!(iter.next(), Some(v2), "second next should yield second value");
        assert_eq!(iter.next(), Some(v3), "third next should yield third value");
        assert_eq!(iter.next(), None, "next on exhausted iterator should return None");
    }

    /// Verify StaticVec IntoIterator drop handles partially consumed iterator.
    #[kani::proof]
    #[kani::unwind(4)]
    fn proof_into_iter_partial_drop() {
        let mut vec: StaticVec<u32, 4> = StaticVec::new();

        let v1: u32 = kani::any();
        let v2: u32 = kani::any();
        let v3: u32 = kani::any();

        vec.push(v1).unwrap();
        vec.push(v2).unwrap();
        vec.push(v3).unwrap();

        let mut iter = vec.into_iter();

        // Consume only first element
        assert_eq!(iter.next(), Some(v1), "should yield first value");

        // Drop iterator with remaining elements - this tests the Drop impl
        // which should properly drop remaining elements v2 and v3
    }

    /// Verify StaticVec truncate operation.
    #[kani::proof]
    #[kani::unwind(4)]
    fn proof_truncate_correctness() {
        let mut vec: StaticVec<u32, 4> = StaticVec::new();

        let v1: u32 = kani::any();
        let v2: u32 = kani::any();
        let v3: u32 = kani::any();

        vec.push(v1).unwrap();
        vec.push(v2).unwrap();
        vec.push(v3).unwrap();

        assert_eq!(vec.len(), 3, "vector should have 3 elements");

        vec.truncate(2);
        assert_eq!(vec.len(), 2, "vector should have 2 elements after truncate");

        let slice: &[u32] = &*vec;
        assert_eq!(slice[0], v1, "first element should remain");
        assert_eq!(slice[1], v2, "second element should remain");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let vec: StaticVec<i32, 10> = StaticVec::new();
        assert_eq!(vec.len, 0);
    }

    #[test]
    fn test_push_pop() {
        let mut vec: StaticVec<i32, 5> = StaticVec::new();

        assert!(vec.push(1).is_ok());
        assert!(vec.push(2).is_ok());
        assert!(vec.push(3).is_ok());
        assert_eq!(vec.len, 3);

        assert_eq!(vec.pop(), Some(3));
        assert_eq!(vec.pop(), Some(2));
        assert_eq!(vec.pop(), Some(1));
        assert_eq!(vec.pop(), None);
    }

    #[test]
    fn test_capacity_exceeded() {
        let mut vec: StaticVec<i32, 2> = StaticVec::new();

        assert!(vec.push(1).is_ok());
        assert!(vec.push(2).is_ok());
        assert!(matches!(vec.push(3), Err(AllocError::OutOfMemory)));
    }

    #[test]
    fn test_deref() {
        let mut vec: StaticVec<i32, 5> = StaticVec::new();
        vec.push(10).unwrap();
        vec.push(20).unwrap();
        vec.push(30).unwrap();

        let slice: &[i32] = &*vec;
        assert_eq!(slice.len(), 3);
    }

    #[test]
    fn test_into_iter() {
        let mut vec: StaticVec<i32, 5> = StaticVec::new();
        vec.push(10).unwrap();
        vec.push(20).unwrap();
        vec.push(30).unwrap();

        let mut iter = vec.into_iter();
        assert_eq!(iter.next(), Some(10));
        assert_eq!(iter.next(), Some(20));
        assert_eq!(iter.next(), Some(30));
        assert_eq!(iter.next(), None);
    }
}
