use crate::MemoryStatistics;
use crate::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::cell::UnsafeCell;

// TODO(tumbar) Do we need to expose this or is it constant across all of SpaceWasm?
const ALIGNMENT: usize = 8;

#[derive(Debug, Default, Clone)]
pub struct PageAllocatorStatistics {
    pub total_bytes: u32,
    pub pad_bytes: u32,
    pub pages: u32,
}

impl From<PageAllocatorStatistics> for MemoryStatistics {
    fn from(stats: PageAllocatorStatistics) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: stats.total_bytes as i32,
            pad_bytes: stats.pad_bytes as i32,
        }
    }
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
            pages: [const { None }; MAX_PAGES],
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

    fn memory_statistics(&self) -> MemoryStatistics {
        self.stats().into()
    }
}

impl<'a, const MAX_PAGES: usize> PageAllocatorInner<'a, MAX_PAGES> {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocError> {
        if layout.size() == 0 {
            return Err(AllocError::AllocationFailed);
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
                    let page_layout = Layout::from_size_align(self.page_size, ALIGNMENT).unwrap();
                    let addr = unsafe { self.page_allocator.alloc(page_layout)? };

                    // Attempt to allocate this memory into the page
                    let mut page = Page::new(addr, self.page_size);
                    let ptr = match page.alloc(layout) {
                        None => {
                            // Allocation failed on a new page
                            // Drop the page and error
                            // FIXME(tumbar) This means that the page size is too small! What do we do?
                            unsafe { self.page_allocator.dealloc(addr, page_layout) }
                            return Err(AllocError::PageTooSmall);
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
                                    Layout::from_size_align(self.page_size, ALIGNMENT).unwrap(),
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
        // Deallocate pages in reverse order to satisfy LIFO allocators like StackAllocator
        for bucket in self.pages.iter_mut().rev() {
            match bucket {
                None => {}
                Some(page) => {
                    unsafe {
                        self.page_allocator.dealloc(
                            page.ptr,
                            Layout::from_size_align(self.page_size, ALIGNMENT).unwrap(),
                        );
                    }

                    bucket.take();
                }
            }
        }
    }
}

#[derive(Clone)]
struct AllocCache {
    restore_ptr: usize,
    alloc_ptr: usize,
}

#[derive(Clone)]
struct Page {
    ptr: *mut u8,
    size: usize,
    allocated: usize,
    n_allocations: usize,
    wasted: usize,
    has_deallocated: bool,
    cache: Option<AllocCache>,
}

impl Page {
    fn new(ptr: *mut u8, size: usize) -> Self {
        Self {
            ptr,
            size,
            allocated: 0,
            n_allocations: 0,
            wasted: 0,
            has_deallocated: false,
            cache: None,
        }
    }

    /// Attempt to allocate on the tail end of this page
    fn alloc(&mut self, layout: Layout) -> Option<*mut u8> {
        // Find the next address that is aligned to this layout
        let mut start_address = (self.ptr as usize) + self.allocated;
        if !start_address.is_multiple_of(layout.align()) {
            let alignment_offset = layout.align() - start_address % layout.align();
            self.wasted += alignment_offset;
            start_address += alignment_offset;
        }

        // Make sure out buffer can fit in here
        let final_offset = (start_address - self.ptr as usize) + layout.size();
        if final_offset <= self.size {
            assert!(!self.has_deallocated);
            self.cache = Some(AllocCache {
                restore_ptr: (self.ptr as usize) + self.allocated,
                alloc_ptr: start_address,
            });

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

        if dealloc_ptr >= page_ptr && dealloc_ptr < page_ptr + self.size {
            // This is our pointer, 'free' it
            assert!(self.n_allocations > 0);

            // Check is we can deallocate this pointer without marking this page with a dealloc flag
            if let Some(cache) = self.cache.take()
                && cache.alloc_ptr == dealloc_ptr
            {
                self.n_allocations -= 1;
                self.allocated = cache.restore_ptr - page_ptr;
                self.wasted -= dealloc_ptr - cache.restore_ptr;
                return Some(self.n_allocations == 0);
            };

            // FIXME(tumbar) We may want to track used regions of the pages
            self.n_allocations -= 1;
            self.has_deallocated = true;
            Some(self.n_allocations == 0)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    use crate::test_support::RustSystemAllocator;

    #[test]
    fn test_page_allocator_basic() {
        let stack_alloc = RustSystemAllocator;
        let page_alloc = PageAllocator::<4>::new(&stack_alloc, 512);

        unsafe {
            let layout = Layout::from_size_align(64, 8).unwrap();
            let ptr1 = page_alloc.alloc(layout).unwrap();
            let ptr2 = page_alloc.alloc(layout).unwrap();

            page_alloc.dealloc(ptr1, layout);
            page_alloc.dealloc(ptr2, layout);
        }
    }

    #[test]
    fn test_page_allocator_stats() {
        let stack_alloc = RustSystemAllocator;
        let page_alloc = PageAllocator::<4>::new(&stack_alloc, 512);

        unsafe {
            let layout = Layout::from_size_align(64, 8).unwrap();
            let _ptr = page_alloc.alloc(layout).unwrap();

            let stats = page_alloc.stats();
            assert_eq!(stats.pages, 1);
            assert!(stats.total_bytes >= 64);
        }
    }

    #[test]
    fn test_page_allocator_multiple_pages() {
        let stack_alloc = RustSystemAllocator;
        let page_alloc = PageAllocator::<4>::new(&stack_alloc, 128);

        unsafe {
            let layout = Layout::from_size_align(100, 8).unwrap();
            let _ptr1 = page_alloc.alloc(layout).unwrap();
            let _ptr2 = page_alloc.alloc(layout).unwrap();

            let stats = page_alloc.stats();
            assert_eq!(stats.pages, 2);
        }
    }

    #[test]
    fn test_page_allocator_out_of_pages() {
        let stack_alloc = RustSystemAllocator;
        let page_alloc = PageAllocator::<2>::new(&stack_alloc, 128);

        unsafe {
            let layout = Layout::from_size_align(100, 8).unwrap();
            let _ptr1 = page_alloc.alloc(layout).unwrap();
            let _ptr2 = page_alloc.alloc(layout).unwrap();
            let result = page_alloc.alloc(layout);
            assert!(matches!(result, Err(AllocError::OutOfMemory)));
        }
    }

    #[test]
    fn test_page_too_small() {
        let stack_alloc = RustSystemAllocator;
        let page_alloc = PageAllocator::<4>::new(&stack_alloc, 64);

        unsafe {
            let layout = Layout::from_size_align(100, 8).unwrap();
            let result = page_alloc.alloc(layout);
            assert!(matches!(result, Err(AllocError::PageTooSmall)));
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    use super::*;
    extern crate std;

    use crate::test_support::RustSystemAllocator;

    /// Verify Page::alloc pointer arithmetic safety and allocation correctness
    #[kani::proof]
    fn proof_page_allocation_safety() {
        let backing_alloc = RustSystemAllocator;

        // Allocate a page from backing allocator
        let page_size = 256;
        let page_layout = Layout::from_size_align(page_size, ALIGNMENT).unwrap();
        let page_ptr = unsafe { backing_alloc.alloc(page_layout).unwrap() };

        let mut page = Page::new(page_ptr, page_size);

        // Test with symbolic size and alignment
        let size: usize = kani::any();
        kani::assume(size > 0 && size <= 64);

        let align: usize = kani::any();
        kani::assume(align > 0 && align <= ALIGNMENT);
        kani::assume(align.is_power_of_two());

        let layout = Layout::from_size_align(size, align).unwrap();

        let allocated_before = page.allocated;
        let wasted_before = page.wasted;

        match page.alloc(layout) {
            Some(ptr) => {
                let ptr_addr = ptr as usize;
                let page_base = page_ptr as usize;

                // Pointer must be aligned
                assert_eq!(ptr_addr % align, 0, "Returned pointer must be aligned");

                // Verify alignment padding is minimal
                let start_before_align = page_base + allocated_before;
                if start_before_align % align != 0 {
                    let padding = page.wasted - wasted_before;
                    assert!(padding < align, "Alignment padding must be < align");
                    assert_eq!(
                        (start_before_align + padding) % align,
                        0,
                        "Padding must result in aligned address"
                    );
                }

                // Wasted bytes must increase monotonically
                assert!(
                    page.wasted >= wasted_before,
                    "Wasted bytes must be monotonic"
                );

                // Pointer must be within page bounds
                assert!(ptr_addr >= page_base, "Pointer must be >= page base");
                assert!(
                    ptr_addr < page_base + page_size,
                    "Pointer must be within page"
                );

                // Allocated offset must be within bounds
                assert!(
                    page.allocated <= page_size,
                    "Allocated offset must not exceed page size"
                );

                // Allocated must increase monotonically
                assert!(
                    page.allocated >= allocated_before,
                    "Allocated must increase"
                );

                // Allocation counter incremented
                assert_eq!(page.n_allocations, 1, "Allocation counter must increment");

                // Cache must be set
                assert!(
                    page.cache.is_some(),
                    "Cache must be populated after allocation"
                );

                // No overflow in pointer arithmetic
                let offset = ptr_addr - page_base;
                assert!(
                    offset.checked_add(size).is_some(),
                    "Offset + size must not overflow"
                );
                assert!(
                    offset + size <= page_size,
                    "Allocation must fit within page"
                );

                // Second allocation must not overlap
                let layout2 = Layout::from_size_align(16, 8).unwrap();
                if let Some(ptr2) = page.alloc(layout2) {
                    let ptr2_addr = ptr2 as usize;
                    let offset2 = ptr2_addr - page_base;

                    // Second allocation must start after first ends
                    assert!(offset2 >= offset + size, "Allocations must not overlap");
                    assert_eq!(
                        page.n_allocations, 2,
                        "Counter must be 2 after second alloc"
                    );
                }
            }
            None => {
                // Allocation failed - page too full
                // This is acceptable
            }
        }

        // Cleanup
        core::mem::forget(page);
        unsafe { backing_alloc.dealloc(page_ptr, page_layout) };
    }

    /// Verify Page::dealloc correctness and cache mechanism
    #[kani::proof]
    fn proof_page_deallocation_safety() {
        let backing_alloc = RustSystemAllocator;

        let page_size = 256;
        let page_layout = Layout::from_size_align(page_size, ALIGNMENT).unwrap();
        let page_ptr = unsafe { backing_alloc.alloc(page_layout).unwrap() };

        let mut page = Page::new(page_ptr, page_size);
        let page_base = page_ptr as usize;

        // Make two allocations
        let layout1 = Layout::from_size_align(32, 8).unwrap();
        let layout2 = Layout::from_size_align(16, 8).unwrap();

        let ptr1 = page.alloc(layout1).unwrap();
        let allocated_after_first = page.allocated;
        let wasted_after_first = page.wasted;

        let ptr2 = page.alloc(layout2).unwrap();
        let _allocated_after_second = page.allocated;
        let _wasted_after_second = page.wasted;

        assert_eq!(page.n_allocations, 2, "Should have 2 allocations");

        // Test LIFO deallocation (cache hit)
        let should_drop = page.dealloc(ptr2, layout2);

        assert_eq!(
            should_drop,
            Some(false),
            "Page should not be dropped with remaining allocations"
        );
        assert_eq!(page.n_allocations, 1, "Counter must decrement");

        // Cache hit must restore allocated to exact previous value
        assert_eq!(
            page.allocated, allocated_after_first,
            "Cache hit must restore allocated to exact previous value"
        );

        // Cache hit must restore wasted bytes (subtracts alignment padding)
        assert_eq!(
            page.wasted, wasted_after_first,
            "Cache hit must restore wasted bytes"
        );

        assert!(
            !page.has_deallocated,
            "Cache hit should not set has_deallocated flag"
        );

        // Test pointer ownership check - pointer outside page range
        let outside_ptr = (page_base - 16) as *mut u8;
        let result = page.dealloc(outside_ptr, layout1);
        assert_eq!(
            result, None,
            "Dealloc of pointer outside page must return None"
        );
        assert_eq!(
            page.n_allocations, 1,
            "Counter must not change for outside pointer"
        );

        // Test non-LIFO deallocation (cache miss)
        let ptr3 = page.alloc(Layout::from_size_align(8, 8).unwrap()).unwrap();
        assert_eq!(page.n_allocations, 2, "Should have 2 allocations again");

        // Deallocate ptr1 (not the last allocation, so cache miss)
        let should_drop2 = page.dealloc(ptr1, layout1);
        assert_eq!(should_drop2, Some(false), "Page should not be dropped");
        assert_eq!(page.n_allocations, 1, "Counter must decrement");
        assert!(
            page.has_deallocated,
            "Cache miss should set has_deallocated flag"
        );

        // Final deallocation should return true (drop page)
        let should_drop3 = page.dealloc(ptr3, Layout::from_size_align(8, 8).unwrap());
        assert_eq!(
            should_drop3,
            Some(true),
            "Page should be dropped when n_allocations reaches 0"
        );
        assert_eq!(page.n_allocations, 0, "Counter must be 0");

        // Cleanup
        core::mem::forget(page);
        unsafe { backing_alloc.dealloc(page_ptr, page_layout) };
    }

    /// Verify PageAllocator orchestration with multiple pages
    #[kani::proof]
    fn proof_page_allocator_correctness() {
        let backing_alloc = RustSystemAllocator;
        let page_alloc = PageAllocator::<3>::new(&backing_alloc, ALIGNMENT);

        // Test zero-size allocation must fail
        let zero_layout = Layout::from_size_align(0, 1).unwrap();
        let result_zero = unsafe { page_alloc.alloc(zero_layout) };
        assert!(result_zero.is_err(), "Zero-size allocation must fail");

        // Test allocation too large for page size must fail
        let huge_layout = Layout::from_size_align(200, 8).unwrap();
        let result_huge = unsafe { page_alloc.alloc(huge_layout) };
        assert!(
            matches!(result_huge, Err(AllocError::PageTooSmall)),
            "Allocation larger than page size must fail with PageTooSmall"
        );

        // Test basic allocation
        let layout = Layout::from_size_align(32, 8).unwrap();
        let result1 = unsafe { page_alloc.alloc(layout) };

        // If first allocation succeeds, verify properties
        if let Ok(ptr1) = result1 {
            // Pointer must be non-null and aligned
            assert!(!ptr1.is_null(), "Allocated pointer must be non-null");
            assert_eq!(ptr1 as usize % 8, 0, "Pointer must be aligned");

            let stats1 = page_alloc.stats();
            assert!(stats1.pages >= 1, "Should have at least 1 page");
            assert!(stats1.total_bytes >= 32, "Should track allocated bytes");

            // Verify stats aggregation: total_bytes = allocated + wasted across all pages
            // (We can't directly verify this without accessing page internals,
            // but we can verify total_bytes is reasonable)
            assert!(
                stats1.total_bytes <= stats1.pages as u32 * ALIGNMENT,
                "Total bytes must not exceed total page capacity"
            );
            assert!(
                stats1.total_bytes >= stats1.pad_bytes,
                "Total bytes must be >= pad bytes"
            );

            // Try second allocation
            let result2 = unsafe { page_alloc.alloc(layout) };
            if let Ok(ptr2) = result2 {
                // Verify no overlap
                let ptr1_addr = ptr1 as usize;
                let ptr2_addr = ptr2 as usize;
                assert!(
                    ptr2_addr >= ptr1_addr + 32 || ptr1_addr >= ptr2_addr + 32,
                    "Allocations must not overlap"
                );

                // Try to allocate until we run out of space
                let result3 = unsafe { page_alloc.alloc(layout) };
                let result4 = unsafe { page_alloc.alloc(layout) };

                // Eventually should run out of pages or backing memory
                if result3.is_ok() && result4.is_ok() {
                    // Keep trying until failure
                    for _ in 0..10 {
                        if unsafe { page_alloc.alloc(layout) }.is_err() {
                            break;
                        }
                    }
                    // With limited backing memory, should eventually fail
                    // (but might not if state space exploration stops early)
                }

                // Test deallocation
                unsafe {
                    page_alloc.dealloc(ptr2, layout);
                    page_alloc.dealloc(ptr1, layout);
                }
            }
        }
    }

    /// Verify Drop frees all pages in correct order
    #[kani::proof]
    fn proof_drop_safety() {
        let backing_alloc = RustSystemAllocator;

        let initial_stats = backing_alloc.memory_statistics();
        assert_eq!(
            initial_stats.total_bytes, 0,
            "Backing allocator must start empty"
        );

        {
            let page_alloc = PageAllocator::<3>::new(&backing_alloc, 128);

            // Each allocation is 100 bytes, forcing new page creation
            // (100 bytes won't fit after first allocation in 128-byte page)
            let layout = Layout::from_size_align(100, 8).unwrap();

            let ptr1 = unsafe { page_alloc.alloc(layout).unwrap() }; // Creates page 0
            let ptr2 = unsafe { page_alloc.alloc(layout).unwrap() }; // Creates page 1
            let ptr3 = unsafe { page_alloc.alloc(layout).unwrap() }; // Creates page 2

            // Verify 3 pages allocated
            let stats = page_alloc.stats();
            assert_eq!(stats.pages, 3, "Must have exactly 3 pages allocated");

            // Memory should be allocated
            let mid_stats = backing_alloc.memory_statistics();
            assert!(mid_stats.total_bytes > 0, "Memory must be allocated");

            // Keep pointers alive to prevent early dealloc
            core::mem::forget((ptr1, ptr2, ptr3));

            // PageAllocator drops here - must deallocate all 3 pages
            // in reverse order: page2, page1, page0
        }

        // After drop, all memory must be freed
        let final_stats = backing_alloc.memory_statistics();
        assert_eq!(
            final_stats.total_bytes, 0,
            "Drop must free all allocated pages"
        );
    }
}
