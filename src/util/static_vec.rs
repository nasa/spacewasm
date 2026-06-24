use crate::ValidationError;
use crate::alloc::AllocError;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};

pub struct StaticVec<T: Sized, const N: usize> {
    data: [MaybeUninit<T>; N],
    len: u32,
}

impl<T: Sized + Clone, const N: usize> Clone for StaticVec<T, N> {
    fn clone(&self) -> Self {
        // Create uninitialized array
        let mut new_vec = StaticVec::<T, N>::default();

        // Clone only the initialized elements
        for i in 0..(self.len as usize) {
            unsafe {
                new_vec.data[i].write(self.data[i].assume_init_ref().clone());
            }
        }

        // Set length after initialization
        new_vec.len = self.len;

        new_vec
    }
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
            data: unsafe { MaybeUninit::uninit().assume_init() },
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

        self.data[self.len as usize].write(value);

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
            unsafe { Some(self.data[self.len as usize].assume_init_read()) }
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
        unsafe {
            // SAFETY: elements [0..len) are initialized
            core::slice::from_raw_parts(self.data.as_ptr() as *const T, self.len as usize)
        }
    }
}

impl<T, const N: usize> DerefMut for StaticVec<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe {
            // SAFETY: elements [0..len) are initialized
            core::slice::from_raw_parts_mut(self.data.as_mut_ptr() as *mut T, self.len as usize)
        }
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
            let item = unsafe { self.vec.data[self.pos].assume_init_read() };
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
            unsafe { self.vec.data[self.pos].assume_init_drop() };
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

        let slice: &[i32] = &vec;
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

    #[test]
    fn test_clone() {
        let mut vec: StaticVec<i32, 5> = StaticVec::new();
        vec.push(1).unwrap();
        vec.push(2).unwrap();
        vec.push(3).unwrap();

        let cloned = vec.clone();
        assert_eq!(cloned.len(), 3);
        assert_eq!(&cloned[..], &[1, 2, 3]);
        assert_eq!(&vec[..], &[1, 2, 3]);
    }
}
