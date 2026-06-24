use core::ops::{Deref, DerefMut};

pub struct InnerVec<T: Sized> {
    pub ptr: *mut T,
    pub capacity: u32,
    pub len: u32,
}

impl<T: core::fmt::Debug> core::fmt::Debug for InnerVec<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: Sized> InnerVec<T> {
    pub fn zero() -> InnerVec<T> {
        InnerVec {
            ptr: core::ptr::null_mut(),
            capacity: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
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

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (**self).iter()
    }
}

impl<T> Deref for InnerVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        if self.ptr.is_null() {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr, self.len as usize) }
        }
    }
}

impl<T> DerefMut for InnerVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        if self.ptr.is_null() {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len as usize) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero() {
        let vec: InnerVec<i32> = InnerVec::zero();
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.capacity(), 0);
        assert!(vec.ptr.is_null());
    }

    #[test]
    fn test_deref_empty() {
        let vec: InnerVec<i32> = InnerVec::zero();
        let slice: &[i32] = &vec;
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_deref_mut_empty() {
        let mut vec: InnerVec<i32> = InnerVec::zero();
        let slice: &mut [i32] = &mut vec;
        assert_eq!(slice.len(), 0);
    }
}
