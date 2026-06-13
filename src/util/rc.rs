use crate::{AllocError, Allocator, GlobalAllocator};
use core::cell::Cell;
use core::hint;
use core::ops::Deref;
use core::panic::{RefUnwindSafe, UnwindSafe};
use core::ptr::NonNull;

struct RcInner<T: ?Sized> {
    count: Cell<usize>,
    value: T,
}

pub struct Rc<T: ?Sized, A: Allocator = GlobalAllocator> {
    ptr: NonNull<RcInner<T>>,
    alloc: A,
}

impl<T: RefUnwindSafe + ?Sized, A: Allocator + UnwindSafe> UnwindSafe for Rc<T, A> {}
impl<T: RefUnwindSafe + ?Sized, A: Allocator + UnwindSafe> RefUnwindSafe for Rc<T, A> {}

impl<T: ?Sized> Deref for Rc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &unsafe { self.ptr.as_ref() }.value
    }
}

impl<T> Rc<T> {
    pub fn new(value: T) -> Result<Rc<T>, AllocError> {
        unsafe {
            Ok(Self::from_inner(
                crate::Box::leak(crate::Box::new(RcInner {
                    count: Cell::new(1),
                    value,
                })?)
                .into(),
            ))
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.inner().count() == 1
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if Rc::is_unique(self) {
            unsafe { Some(Rc::get_mut_unchecked(self)) }
        } else {
            None
        }
    }

    #[inline]
    pub unsafe fn get_mut_unchecked(&mut self) -> &mut T {
        // We are careful to *not* create a reference covering the "count" fields, as
        // this would conflict with accesses to the reference counts (e.g. by `Weak`).
        unsafe { &mut (*self.ptr.as_ptr()).value }
    }
}

impl<T: ?Sized, A: Allocator + Clone> Clone for Rc<T, A> {
    /// Makes a clone of the `Rc` pointer.
    ///
    /// This creates another pointer to the same allocation, increasing the
    /// strong reference count.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    ///
    /// let five = Rc::new(5);
    ///
    /// let _ = Rc::clone(&five);
    /// ```
    #[inline]
    fn clone(&self) -> Self {
        unsafe {
            self.inner().inc();
            Self::from_inner_in(self.ptr, self.alloc.clone())
        }
    }
}

impl<T: ?Sized> Rc<T> {
    #[inline]
    unsafe fn from_inner(ptr: NonNull<RcInner<T>>) -> Self {
        unsafe { Self::from_inner_in(ptr, GlobalAllocator) }
    }
}

impl<T: ?Sized, A: Allocator> Rc<T, A> {
    #[inline(always)]
    fn inner(&self) -> &RcInner<T> {
        // This unsafety is ok because while this Rc is alive we're guaranteed
        // that the inner pointer is valid.
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    unsafe fn from_inner_in(ptr: NonNull<RcInner<T>>, alloc: A) -> Self {
        Self { ptr, alloc }
    }
}

impl<T: ?Sized> RcInner<T> {
    #[inline]
    fn count(&self) -> usize {
        self.count.get()
    }

    #[inline]
    fn inc(&self) {
        let count = self.count();

        // We insert an `assume` here to hint LLVM at an otherwise
        // missed optimization.
        // SAFETY: The reference count will never be zero when this is
        // called.
        unsafe {
            hint::assert_unchecked(count != 0);
        }

        let strong = count.wrapping_add(1);
        self.count.set(strong);

        // We want to abort on overflow instead of dropping the value.
        // Checking for overflow after the store instead of before
        // allows for slightly better code generation.
        assert_ne!(count, 0);
    }
}
