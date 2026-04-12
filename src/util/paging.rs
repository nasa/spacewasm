use crate::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::cell::UnsafeCell;

#[derive(Debug, Default, Clone)]
pub struct PageAllocatorStatistics {
    pub total_bytes: u32,
    pub pad_bytes: u32,
    pub pages: u32,
}

/// A page is an allocator that utilizes a large contiguous blocks of memory
/// to perform smaller allocations. It is strictly increasing in it's offset for
/// simplicity. This means a bounded number of allocations should occur since it does
/// no garbage collection within the page. If all allocations within a single page are
/// freed it will also free the page.
///
/// Page allocators wrap another allocator who is responsible for actually allocating
/// each page. The page allocator will only call to this allocator once it can no longer
/// fit the next allocation in any of it's currently allocated pages.
///
/// This allocator supports a static number of pages and therefore can run out of memory
pub struct PageAllocator<'a, const MAX_PAGES: usize> {
    inner: UnsafeCell<PageAllocatorInner<'a, MAX_PAGES>>,
}

impl<'a, const MAX_PAGES: usize> PageAllocator<'a, MAX_PAGES> {
    pub const fn new(alloc: &'a dyn Allocator, page_size: usize) -> Self {
        Self {
            inner: UnsafeCell::new(PageAllocatorInner::new(alloc, page_size)),
        }
    }

    pub fn stats(&self) -> PageAllocatorStatistics {
        let inner = unsafe { &*self.inner.get() };
        let mut stats = PageAllocatorStatistics::default();
        for bucket in &inner.pages {
            match bucket {
                None => {}
                Some(page) => {
                    stats.total_bytes += page.allocated as u32;
                    stats.pad_bytes += page.wasted as u32;
                    stats.pages += 1;
                }
            }
        }

        stats
    }
}

struct PageAllocatorInner<'a, const MAX_PAGES: usize> {
    page_allocator: &'a dyn Allocator,
    page_size: usize,
    pages: [Option<Page>; MAX_PAGES],
}

impl<'a, const MAX_PAGES: usize> PageAllocatorInner<'a, MAX_PAGES> {
    const fn new(alloc: &'a dyn Allocator, page_size: usize) -> PageAllocatorInner<'a, MAX_PAGES> {
        PageAllocatorInner {
            page_allocator: alloc,
            page_size,
            pages: [None; MAX_PAGES],
        }
    }
}

unsafe impl<'a, const MAX_PAGES: usize> Allocator for PageAllocator<'a, MAX_PAGES> {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe { (&mut *self.inner.get()).alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { (&mut *self.inner.get()).dealloc(ptr, layout) }
    }
}

impl<'a, const MAX_PAGES: usize> PageAllocatorInner<'a, MAX_PAGES> {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocError> {
        if layout.size() == 0 {
            return Err(AllocError::IllegalZeroSize);
        }

        // Go through each page one-by-one and try to allocate
        // If we reach a 'None' page, allocate the page and allocate this request there
        for bucket in self.pages.iter_mut() {
            match bucket {
                Some(page) => {
                    // Attempt to allocate to this page
                    match page.alloc(layout) {
                        None => {
                            // Allocation failed, fallthrough to the next page
                        }
                        Some(ptr) => {
                            return Ok(ptr);
                        }
                    }
                }
                None => {
                    // We have reached an empty page
                    // Allocate the page and place the allocation here
                    let page_layout = Layout::from_size_align(self.page_size, 128)?;
                    let addr = unsafe { self.page_allocator.alloc(page_layout)? };

                    // Attempt to allocate this memory into the page
                    let mut page = Page::new(addr, self.page_size);
                    let ptr = match page.alloc(layout) {
                        None => {
                            // Allocation failed on a new page
                            // Drop the page and error
                            // FIXME(tumbar) This means that the page size is too small! What do we do?
                            unsafe { self.page_allocator.dealloc(addr, page_layout) }
                            return Err(AllocError::PageTooSmall(layout.size()));
                        }
                        Some(ptr) => ptr,
                    };

                    // Place the new page in the bucket
                    bucket.replace(page);
                    return Ok(ptr);
                }
            }
        }

        // Does not fit
        Err(AllocError::OutOfMemory)
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        for bucket in self.pages.iter_mut() {
            if let Some(page) = bucket {
                match page.dealloc(ptr, layout) {
                    None => {
                        // This ptr is not from this page
                        // Fallthrough to the next page
                    }
                    Some(drop_page) => {
                        if drop_page {
                            unsafe {
                                self.page_allocator.dealloc(
                                    page.ptr,
                                    Layout::from_size_align(self.page_size, 128).unwrap(),
                                );
                            }

                            bucket.take();
                        }

                        return;
                    }
                }
            }
        }
    }
}

impl<'a, const MAX_PAGES: usize> Drop for PageAllocatorInner<'a, MAX_PAGES> {
    fn drop(&mut self) {
        for bucket in self.pages.iter_mut() {
            match bucket {
                None => {}
                Some(page) => {
                    unsafe {
                        self.page_allocator.dealloc(
                            page.ptr,
                            Layout::from_size_align(self.page_size, 128).unwrap(),
                        );
                    }

                    bucket.take();
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Page {
    ptr: *mut u8,
    size: usize,
    allocated: usize,
    n_allocations: usize,
    wasted: usize,
}

impl Page {
    fn new(ptr: *mut u8, size: usize) -> Self {
        Self {
            ptr,
            size,
            allocated: 0,
            n_allocations: 0,
            wasted: 0,
        }
    }

    /// Attempt to allocate on the tail end of this page
    fn alloc(&mut self, layout: Layout) -> Option<*mut u8> {
        // Find the next address that is aligned to this layout
        let mut start_address = (self.ptr as usize) + self.allocated;
        if start_address % layout.align() > 0 {
            let alignment_offset = layout.align() - start_address % layout.align();
            self.wasted += alignment_offset;
            start_address = start_address + alignment_offset;
        }

        // Make sure out buffer can fit in here
        let final_offset = (start_address - self.ptr as usize) + layout.size();
        if final_offset <= self.size {
            self.allocated = final_offset;
            self.n_allocations += 1;
            Some(start_address as *mut u8)
        } else {
            None
        }
    }

    fn dealloc(&mut self, ptr: *mut u8, _layout: Layout) -> Option<bool> {
        // Check if this pointer is ours
        let dealloc_ptr = ptr as usize;
        let page_ptr = self.ptr as usize;

        if page_ptr <= dealloc_ptr && dealloc_ptr <= page_ptr + self.size {
            // This is out pointer, 'free' it
            // FIXME(tumbar) We may want to track used regions of the pages
            self.n_allocations -= 1;
            Some(self.n_allocations == 0)
        } else {
            None
        }
    }
}
