//! What loading a module costs the `spacewasm` global heap, and whether that fits a
//! flight heap.
//!
//! The decoder allocates through the process-global allocator the binary installed, which
//! this library cannot see into. [`HeapRecorder`] wraps it to log every allocation made
//! while [`load`](crate::load) runs, and the load marks where each section starts, so
//! each allocation is charged to the section being decoded when it was made. The log is
//! then replayed through a `PageAllocator` of the flight heap's page size, the allocator
//! the flight heap is, so pages and padding come out as they would on board.

use spacewasm::{AllocError, Allocator, PageAllocator};
use std::alloc::Layout;
use std::cell::RefCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use wasmparser::BinaryReader;

thread_local! {
    /// Allocations since [`start`], while a load records; `None` otherwise, so the
    /// commands that run modules do not log every allocation they make.
    static LOG: RefCell<Option<Vec<Layout>>> = const { RefCell::new(None) };
}

/// Whether the global allocator is a [`HeapRecorder`]: set by its first allocation, which
/// the engine's stack makes before any module is read.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The binary's global allocator, logging what it hands out while a load records:
///
/// ```ignore
/// spacewasm::global_allocator!(
///     HeapRecorder<PageAllocator<System, 0x200>>,
///     HeapRecorder::new(PageAllocator::new(System, 0x8_000_000))
/// );
/// ```
///
/// Without it a load still succeeds, and [`Loaded::heap`](crate::Loaded::heap) is `None`.
pub struct HeapRecorder<A>(A);

impl<A: Allocator> HeapRecorder<A> {
    pub const fn new(inner: A) -> Self {
        HeapRecorder(inner)
    }
}

// SAFETY: forwards to the inner allocator, which upholds the same contract.
unsafe impl<A: Allocator> Allocator for HeapRecorder<A> {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        // SAFETY: the caller upholds `alloc`'s contract, which is the inner allocator's.
        let ptr = unsafe { self.0.alloc(layout) }?;
        INSTALLED.store(true, Ordering::Relaxed);
        // `try_with`: an allocation while the thread is torn down is just not logged.
        let _ = LOG.try_with(|log| {
            if let Some(log) = log.borrow_mut().as_mut() {
                log.push(layout);
            }
        });
        Ok(ptr)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as for `alloc`.
        unsafe { self.0.dealloc(ptr, layout) }
    }
}

/// Start logging this thread's allocations, from empty.
pub(crate) fn start() {
    LOG.with_borrow_mut(|log| *log = Some(Vec::new()));
}

/// Allocations logged so far.
pub(crate) fn logged() -> usize {
    LOG.with_borrow(|log| log.as_ref().map_or(0, Vec::len))
}

/// Stop logging, and take what was logged: `None` if nothing could have been, because
/// the global allocator is not a [`HeapRecorder`].
pub(crate) fn stop() -> Option<Vec<Layout>> {
    let log = LOG.with_borrow_mut(Option::take);
    INSTALLED
        .load(Ordering::Relaxed)
        .then(|| log.unwrap_or_default())
}

/// What a module's load allocated on the global heap, section by section. The engine's
/// own allocations (its stack, the store, the code builder's page table) are made
/// before the module is read, are the embedder's, and are not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heap {
    pub sections: Vec<SectionHeap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionHeap {
    /// `type`, `code`, `custom "name"` and so on; `header` for the module's preamble and
    /// `instantiate` for what was allocated after its last section.
    pub name: String,
    /// The section's size in the module, its id and size included.
    pub size: usize,
    /// Every allocation charged to it, in order.
    pub allocations: Vec<Layout>,
}

/// A [`Heap`] laid out on pages of one size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeapPages {
    pub page_size: usize,
    /// Pages the module's allocations took.
    pub pages: usize,
    /// One per [`Heap::sections`], in the same order.
    pub sections: Vec<PageUse>,
}

/// Heap one section took, alignment padding included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageUse {
    pub bytes: usize,
    /// Of `bytes`, the padding alignment cost.
    pub padding: usize,
}

impl HeapPages {
    pub fn bytes(&self) -> usize {
        self.sections.iter().map(|section| section.bytes).sum()
    }

    pub fn padding(&self) -> usize {
        self.sections.iter().map(|section| section.padding).sum()
    }
}

/// Why a [`Heap`] does not fit its pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    /// A single allocation is larger than a page, so no number of pages holds it.
    TooLarge {
        section: String,
        size: usize,
        page_size: usize,
    },
    /// More pages than are simulated.
    TooManyPages { page_size: usize },
    /// The pages themselves could not be allocated.
    Alloc(AllocError),
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::TooLarge {
                section,
                size,
                page_size,
            } => write!(
                f,
                "the {section} section allocates {size} bytes at once, more than a \
                 {page_size}-byte heap page holds"
            ),
            HeapError::TooManyPages { page_size } => write!(
                f,
                "the module needs more than {SIMULATED_PAGES} heap pages of {page_size} bytes"
            ),
            HeapError::Alloc(err) => write!(f, "could not lay out the heap: {err:?}"),
        }
    }
}

impl std::error::Error for HeapError {}

/// Pages a layout is simulated on; a module needing more is reported as needing more.
const SIMULATED_PAGES: usize = 1024;

impl Heap {
    /// Replay every allocation, in order, through a `PageAllocator` of `page_size`-byte
    /// pages.
    pub fn on_pages(&self, page_size: usize) -> Result<HeapPages, HeapError> {
        let heap = PageAllocator::<StdPages, SIMULATED_PAGES>::new(StdPages, page_size);
        let mut before = heap.stats();
        let mut sections = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            for &layout in &section.allocations {
                // SAFETY: every layout logged was a successful allocation, so not zero-sized.
                match unsafe { heap.alloc(layout) } {
                    Ok(_) => {}
                    Err(AllocError::PageTooSmall) => {
                        return Err(HeapError::TooLarge {
                            section: section.name.clone(),
                            size: layout.size(),
                            page_size,
                        });
                    }
                    Err(AllocError::OutOfMemory) => {
                        return Err(HeapError::TooManyPages { page_size });
                    }
                    Err(err) => return Err(HeapError::Alloc(err)),
                }
            }
            // Nothing is freed, so both only grow.
            let after = heap.stats();
            sections.push(PageUse {
                bytes: (after.total_bytes - before.total_bytes) as usize,
                padding: (after.pad_bytes - before.pad_bytes) as usize,
            });
            before = after;
        }
        Ok(HeapPages {
            page_size,
            pages: before.pages as usize,
            sections,
        })
    }
}

/// Pages for the simulated heap, from the Rust global allocator.
struct StdPages;

// SAFETY: forwards to the Rust global allocator, which upholds the same contract.
unsafe impl Allocator for StdPages {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        // SAFETY: the caller guarantees a non-zero size, as `Allocator` requires.
        let ptr = unsafe { std::alloc::alloc(layout) };
        match ptr.is_null() {
            true => Err(AllocError::AllocationFailed),
            false => Ok(ptr),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `alloc` with `layout`.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

/// Where each section starts, and its name: the preamble first, as `header`. Stops at the
/// first malformed section, which the decoder will refuse anyway.
pub(crate) fn sections(module: &[u8]) -> Vec<(usize, String)> {
    let mut sections = vec![(0, "header".to_string())];
    let mut reader = BinaryReader::new(module, 0);
    if reader.read_bytes(8).is_err() {
        return sections;
    }
    while !reader.eof() {
        let start = reader.original_position() as usize;
        let Ok(id) = reader.read_u8() else { break };
        let Ok(size) = reader.read_var_u32() else {
            break;
        };
        let Ok(contents) = reader.read_bytes(size as usize) else {
            break;
        };
        sections.push((start, name(id, contents)));
    }
    sections
}

fn name(id: u8, contents: &[u8]) -> String {
    match id {
        0 => match BinaryReader::new(contents, 0).read_string() {
            Ok(name) => format!("custom {name:?}"),
            Err(_) => "custom".to_string(),
        },
        1 => "type".to_string(),
        2 => "import".to_string(),
        3 => "function".to_string(),
        4 => "table".to_string(),
        5 => "memory".to_string(),
        6 => "global".to_string(),
        7 => "export".to_string(),
        8 => "start".to_string(),
        9 => "element".to_string(),
        10 => "code".to_string(),
        11 => "data".to_string(),
        12 => "data count".to_string(),
        id => format!("section {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heap(allocations: &[(usize, usize)]) -> Heap {
        Heap {
            sections: vec![SectionHeap {
                name: "code".into(),
                size: 0,
                allocations: allocations
                    .iter()
                    .map(|&(size, align)| Layout::from_size_align(size, align).unwrap())
                    .collect(),
            }],
        }
    }

    #[test]
    fn fills_a_page_before_taking_another() {
        let pages = heap(&[(40, 8), (24, 8), (8, 8)]).on_pages(64).unwrap();
        assert_eq!(pages.pages, 2);
        assert_eq!(pages.bytes(), 72);
        assert_eq!(pages.padding(), 0);
    }

    #[test]
    fn counts_alignment_padding() {
        // A page is at least 8-aligned, so the second allocation starts 7 bytes later.
        let pages = heap(&[(1, 1), (8, 8)]).on_pages(64).unwrap();
        assert_eq!(pages.pages, 1);
        assert_eq!(pages.bytes(), 16);
        assert_eq!(pages.padding(), 7);
    }

    #[test]
    fn an_allocation_larger_than_a_page_never_fits() {
        assert_eq!(
            heap(&[(65, 8)]).on_pages(64),
            Err(HeapError::TooLarge {
                section: "code".into(),
                size: 65,
                page_size: 64
            })
        );
    }

    #[test]
    fn names_each_section() {
        let module = wat::parse_str(r#"(module (func (export "f")) (@custom "c" "x"))"#).unwrap();
        let names: Vec<_> = sections(&module)
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(
            names,
            [
                "header",
                "type",
                "function",
                "export",
                "code",
                "custom \"c\""
            ]
        );
    }
}
