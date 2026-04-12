use core::ops::{Deref, DerefMut};

pub struct StackVec<T: Sized, const N: usize> {
    data: [T; N],
    len: usize,
}

impl<T, const N: usize> StackVec<T, N> {
    pub fn new() -> Self {
        Self {
            data: unsafe { core::mem::zeroed() },
            len: 0,
        }
    }

    pub fn push(&mut self, value: T) {
        assert!(self.len < N);

        unsafe {
            core::ptr::write(&mut self.data[self.len], value);
        }

        self.len += 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(core::ptr::read(&self.data[self.len])) }
        }
    }
}

impl<T: core::fmt::Debug, const N: usize> core::fmt::Debug for StackVec<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T, const N: usize> Deref for StackVec<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.data
    }
}

impl<T, const N: usize> DerefMut for StackVec<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data
    }
}
