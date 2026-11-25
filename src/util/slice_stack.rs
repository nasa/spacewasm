use crate::alloc::AllocError;
use core::ops::{Deref, DerefMut};

/// A stack over borrowed storage: `StaticVec` without owning its capacity.
///
/// Elements are `Copy` and the backing slice is always initialized, so pushing
/// overwrites a slot and popping only moves the length.
pub(crate) struct SliceStack<'a, T: Copy> {
    data: &'a mut [T],
    len: usize,
}

impl<'a, T: Copy> SliceStack<'a, T> {
    pub fn new(data: &'a mut [T]) -> Self {
        Self { data, len: 0 }
    }

    pub fn push(&mut self, value: T) -> Result<(), AllocError> {
        let slot = self.data.get_mut(self.len).ok_or(AllocError::OutOfMemory)?;
        *slot = value;
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            Some(self.data[self.len])
        }
    }

    pub fn truncate(&mut self, new_len: usize) {
        assert!(new_len <= self.len);
        self.len = new_len;
    }
}

impl<T: Copy> Deref for SliceStack<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.data[..self.len]
    }
}

impl<T: Copy> DerefMut for SliceStack<'_, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Push and pop keep LIFO order and stop at the borrowed capacity.
    #[test]
    fn push_pop_respects_capacity() {
        let mut storage = [0u32; 2];
        let mut stack = SliceStack::new(&mut storage);
        assert!(stack.push(1).is_ok());
        assert!(stack.push(2).is_ok());
        assert_eq!(stack.push(3), Err(AllocError::OutOfMemory));
        assert_eq!(&*stack, &[1, 2]);
        assert_eq!(stack.pop(), Some(2));
        stack.truncate(0);
        assert_eq!(stack.pop(), None);
    }

    /// A zero-length backing slice refuses every push.
    #[test]
    fn empty_storage_refuses_push() {
        let mut stack = SliceStack::<u32>::new(&mut []);
        assert_eq!(stack.push(1), Err(AllocError::OutOfMemory));
    }
}
