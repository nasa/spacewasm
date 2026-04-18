use core::mem::MaybeUninit;
use core::ptr;

/// A fixed-size circular buffer.
///
/// A `CircularBuffer` may live on the stack with a fixed size determined at compile time.
/// When the buffer is full, pushing new elements will overwrite the oldest elements.
///
/// See the [module-level documentation](self) for more details and examples.
pub struct CircularBuffer<T, const N: usize> {
    size: usize,
    start: usize,
    items: [MaybeUninit<T>; N],
}

impl<T, const N: usize> CircularBuffer<T, N> {
    /// Creates a new empty circular buffer.
    pub const fn new() -> Self {
        Self {
            size: 0,
            start: 0,
            items: unsafe { MaybeUninit::uninit().assume_init() },
        }
    }

    /// Returns the number of elements in the buffer.
    pub const fn len(&self) -> usize {
        self.size
    }

    /// Returns `true` if the buffer contains no elements.
    pub const fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Returns `true` if the buffer is full.
    pub const fn is_full(&self) -> bool {
        self.size == N
    }

    /// Returns the capacity of the buffer.
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Pushes an element to the back of the buffer.
    /// If the buffer is full, the oldest element is overwritten.
    pub fn push(&mut self, value: T) {
        if self.size < N {
            let index = (self.start + self.size) % N;
            self.items[index].write(value);
            self.size += 1;
        } else {
            // Buffer is full, overwrite oldest element
            let index = self.start;
            unsafe {
                ptr::drop_in_place(self.items[index].as_mut_ptr());
            }
            self.items[index].write(value);
            self.start = (self.start + 1) % N;
        }
    }

    /// Removes and returns the element at the front of the buffer.
    pub fn pop_front(&mut self) -> Option<T> {
        if self.size == 0 {
            return None;
        }

        let value = unsafe { self.items[self.start].assume_init_read() };
        self.start = (self.start + 1) % N;
        self.size -= 1;
        Some(value)
    }

    /// Removes and returns the element at the back of the buffer.
    pub fn pop_back(&mut self) -> Option<T> {
        if self.size == 0 {
            return None;
        }

        self.size -= 1;
        let index = (self.start + self.size) % N;
        Some(unsafe { self.items[index].assume_init_read() })
    }

    /// Returns a reference to the element at the front of the buffer.
    pub fn front(&self) -> Option<&T> {
        if self.size == 0 {
            return None;
        }
        Some(unsafe { self.items[self.start].assume_init_ref() })
    }

    /// Returns a mutable reference to the element at the front of the buffer.
    pub fn front_mut(&mut self) -> Option<&mut T> {
        if self.size == 0 {
            return None;
        }
        Some(unsafe { self.items[self.start].assume_init_mut() })
    }

    /// Returns a reference to the element at the back of the buffer.
    pub fn back(&self) -> Option<&T> {
        if self.size == 0 {
            return None;
        }
        let index = (self.start + self.size - 1) % N;
        Some(unsafe { self.items[index].assume_init_ref() })
    }

    /// Returns a mutable reference to the element at the back of the buffer.
    pub fn back_mut(&mut self) -> Option<&mut T> {
        if self.size == 0 {
            return None;
        }
        let index = (self.start + self.size - 1) % N;
        Some(unsafe { self.items[index].assume_init_mut() })
    }

    /// Returns a reference to an element at the given index.
    /// Index 0 is the front of the buffer.
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.size {
            return None;
        }
        let actual_index = (self.start + index) % N;
        Some(unsafe { self.items[actual_index].assume_init_ref() })
    }

    /// Returns a mutable reference to an element at the given index.
    /// Index 0 is the front of the buffer.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.size {
            return None;
        }
        let actual_index = (self.start + index) % N;
        Some(unsafe { self.items[actual_index].assume_init_mut() })
    }

    /// Clears the buffer, removing all values.
    pub fn clear(&mut self) {
        while self.pop_front().is_some() {}
    }

    /// Returns an iterator over the buffer.
    pub fn iter(&self) -> Iter<'_, T, N> {
        Iter {
            buffer: self,
            index: 0,
        }
    }

    /// Returns an iterator that allows modifying each value.
    pub fn iter_mut(&mut self) -> IterMut<'_, T, N> {
        IterMut {
            buffer: self,
            index: 0,
        }
    }
}

impl<T, const N: usize> Drop for CircularBuffer<T, N> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T, const N: usize> Default for CircularBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// An iterator over the elements of a `CircularBuffer`.
pub struct Iter<'a, T, const N: usize> {
    buffer: &'a CircularBuffer<T, N>,
    index: usize,
}

impl<'a, T, const N: usize> Iterator for Iter<'a, T, N> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.buffer.size {
            let value = self.buffer.get(self.index);
            self.index += 1;
            value
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.buffer.size - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, T, const N: usize> ExactSizeIterator for Iter<'a, T, N> {}

/// A mutable iterator over the elements of a `CircularBuffer`.
pub struct IterMut<'a, T, const N: usize> {
    buffer: &'a mut CircularBuffer<T, N>,
    index: usize,
}

impl<'a, T, const N: usize> Iterator for IterMut<'a, T, N> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.buffer.size {
            let actual_index = (self.buffer.start + self.index) % N;
            self.index += 1;
            // SAFETY: We're creating a mutable reference with a limited lifetime.
            // The buffer won't be modified during iteration.
            Some(unsafe { &mut *self.buffer.items[actual_index].as_mut_ptr() })
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.buffer.size - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, T, const N: usize> ExactSizeIterator for IterMut<'a, T, N> {}

impl<'a, T, const N: usize> IntoIterator for &'a CircularBuffer<T, N> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T, N>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut CircularBuffer<T, N> {
    type Item = &'a mut T;
    type IntoIter = IterMut<'a, T, N>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buffer: CircularBuffer<i32, 5> = CircularBuffer::new();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
        assert!(!buffer.is_full());
        assert_eq!(buffer.capacity(), 5);
    }

    #[test]
    fn test_push_and_pop_front() {
        let mut buffer: CircularBuffer<i32, 3> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        assert_eq!(buffer.len(), 3);
        assert!(buffer.is_full());

        assert_eq!(buffer.pop_front(), Some(1));
        assert_eq!(buffer.pop_front(), Some(2));
        assert_eq!(buffer.pop_front(), Some(3));
        assert_eq!(buffer.pop_front(), None);

        assert!(buffer.is_empty());
    }

    #[test]
    fn test_push_and_pop_back() {
        let mut buffer: CircularBuffer<i32, 3> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        assert_eq!(buffer.pop_back(), Some(3));
        assert_eq!(buffer.pop_back(), Some(2));
        assert_eq!(buffer.pop_back(), Some(1));
        assert_eq!(buffer.pop_back(), None);
    }

    #[test]
    fn test_overflow_overwrites_oldest() {
        let mut buffer: CircularBuffer<i32, 3> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        assert_eq!(buffer.len(), 3);

        // Pushing a 4th element should overwrite the oldest (1)
        buffer.push(4);
        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.pop_front(), Some(2));
        assert_eq!(buffer.pop_front(), Some(3));
        assert_eq!(buffer.pop_front(), Some(4));
        assert_eq!(buffer.pop_front(), None);
    }

    #[test]
    fn test_overflow_multiple_times() {
        let mut buffer: CircularBuffer<i32, 3> = CircularBuffer::new();

        for i in 1..=10 {
            buffer.push(i);
        }

        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.pop_front(), Some(8));
        assert_eq!(buffer.pop_front(), Some(9));
        assert_eq!(buffer.pop_front(), Some(10));
    }

    #[test]
    fn test_front_and_back() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        assert_eq!(buffer.front(), None);
        assert_eq!(buffer.back(), None);

        buffer.push(1);
        assert_eq!(buffer.front(), Some(&1));
        assert_eq!(buffer.back(), Some(&1));

        buffer.push(2);
        buffer.push(3);
        assert_eq!(buffer.front(), Some(&1));
        assert_eq!(buffer.back(), Some(&3));
    }

    #[test]
    fn test_front_mut_and_back_mut() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        if let Some(front) = buffer.front_mut() {
            *front = 10;
        }
        if let Some(back) = buffer.back_mut() {
            *back = 30;
        }

        assert_eq!(buffer.pop_front(), Some(10));
        assert_eq!(buffer.pop_back(), Some(30));
        assert_eq!(buffer.pop_front(), Some(2));
    }

    #[test]
    fn test_get() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(10);
        buffer.push(20);
        buffer.push(30);

        assert_eq!(buffer.get(0), Some(&10));
        assert_eq!(buffer.get(1), Some(&20));
        assert_eq!(buffer.get(2), Some(&30));
        assert_eq!(buffer.get(3), None);
    }

    #[test]
    fn test_get_mut() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(10);
        buffer.push(20);
        buffer.push(30);

        if let Some(val) = buffer.get_mut(1) {
            *val = 200;
        }

        assert_eq!(buffer.get(0), Some(&10));
        assert_eq!(buffer.get(1), Some(&200));
        assert_eq!(buffer.get(2), Some(&30));
    }

    #[test]
    fn test_get_after_wrap() {
        let mut buffer: CircularBuffer<i32, 3> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4); // Wraps, buffer now contains [2, 3, 4]

        assert_eq!(buffer.get(0), Some(&2));
        assert_eq!(buffer.get(1), Some(&3));
        assert_eq!(buffer.get(2), Some(&4));
    }

    #[test]
    fn test_clear() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        assert_eq!(buffer.len(), 3);
        buffer.clear();
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
        assert_eq!(buffer.pop_front(), None);
    }

    #[test]
    fn test_iter() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        let mut iter = buffer.iter();
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_iter_after_wrap() {
        let mut buffer: CircularBuffer<i32, 3> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);
        buffer.push(4);
        buffer.push(5); // Buffer contains [3, 4, 5]

        let mut iter = buffer.iter();
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&5));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_iter_mut() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        for val in buffer.iter_mut() {
            *val *= 10;
        }

        let mut iter = buffer.iter();
        assert_eq!(iter.next(), Some(&10));
        assert_eq!(iter.next(), Some(&20));
        assert_eq!(iter.next(), Some(&30));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_iter_empty() {
        let buffer: CircularBuffer<i32, 5> = CircularBuffer::new();
        let mut iter = buffer.iter();
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_exact_size_iterator() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        let iter = buffer.iter();
        assert_eq!(iter.len(), 3);
    }

    #[test]
    fn test_into_iter_ref() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        let mut iter = (&buffer).into_iter();
        assert_eq!(iter.next(), Some(&1));
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_into_iter_mut() {
        let mut buffer: CircularBuffer<i32, 5> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        for val in &mut buffer {
            *val += 1;
        }

        let mut iter = buffer.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_drop_cleans_up() {
        // This test verifies that Drop is called on all elements
        use core::cell::Cell;

        struct DropCounter<'a> {
            counter: &'a Cell<usize>,
        }

        impl<'a> Drop for DropCounter<'a> {
            fn drop(&mut self) {
                self.counter.set(self.counter.get() + 1);
            }
        }

        let counter = Cell::new(0);

        {
            let mut buffer: CircularBuffer<DropCounter, 3> = CircularBuffer::new();
            buffer.push(DropCounter { counter: &counter });
            buffer.push(DropCounter { counter: &counter });
            buffer.push(DropCounter { counter: &counter });
        } // Buffer dropped here

        assert_eq!(counter.get(), 3);
    }

    #[test]
    fn test_drop_with_overflow() {
        use core::cell::Cell;

        struct DropCounter<'a> {
            counter: &'a Cell<usize>,
        }

        impl<'a> Drop for DropCounter<'a> {
            fn drop(&mut self) {
                self.counter.set(self.counter.get() + 1);
            }
        }

        let counter = Cell::new(0);

        {
            let mut buffer: CircularBuffer<DropCounter, 2> = CircularBuffer::new();
            buffer.push(DropCounter { counter: &counter });
            buffer.push(DropCounter { counter: &counter });
            buffer.push(DropCounter { counter: &counter }); // Overwrites first, drops it
        } // Buffer dropped here, drops remaining 2

        // Total: 1 (overwritten) + 2 (final drop) = 3
        assert_eq!(counter.get(), 3);
    }

    #[test]
    fn test_mixed_operations() {
        let mut buffer: CircularBuffer<i32, 4> = CircularBuffer::new();

        buffer.push(1);
        buffer.push(2);
        assert_eq!(buffer.pop_front(), Some(1));

        buffer.push(3);
        buffer.push(4);
        buffer.push(5);

        // After pop_front, we had 1 element, then pushed 3 more = 4 total (buffer is now full)
        assert_eq!(buffer.len(), 4);
        assert_eq!(buffer.get(0), Some(&2));
        assert_eq!(buffer.get(1), Some(&3));
        assert_eq!(buffer.get(2), Some(&4));
        assert_eq!(buffer.get(3), Some(&5));

        assert_eq!(buffer.pop_back(), Some(5));
        buffer.push(6);

        let mut iter = buffer.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&6));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_single_element_buffer() {
        let mut buffer: CircularBuffer<i32, 1> = CircularBuffer::new();

        buffer.push(1);
        assert!(buffer.is_full());
        assert_eq!(buffer.front(), Some(&1));
        assert_eq!(buffer.back(), Some(&1));

        buffer.push(2);
        assert_eq!(buffer.front(), Some(&2));
        assert_eq!(buffer.pop_front(), Some(2));
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_default() {
        let buffer: CircularBuffer<i32, 5> = Default::default();
        assert!(buffer.is_empty());
        assert_eq!(buffer.capacity(), 5);
    }
}
