use crate::alloc::AllocError;
use crate::ValidationError;
use core::ops::{Deref, DerefMut};

pub struct StaticVec<T: Sized, const N: usize> {
    data: [T; N],
    len: u32,
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
