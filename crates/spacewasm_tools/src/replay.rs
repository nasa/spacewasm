//! Decoding a module again, the way the flight interpreter did, to record its IR layout.
//!
//! A PC only means something against the IR the flight decoder produced, and that
//! depends on the compiler options and on how every import resolved: a host call and a
//! guest call compile to different instructions. So the replay needs the same host
//! modules the flight store was built with. Their bodies are never called; only the
//! names and signatures matter.
//!
//! `spacewasm` allocates through a process-global allocator. A library must not install
//! one, so this runs under whatever the binary installed
//! (`spacewasm::global_allocator!`), and is no more re-entrant than that allocator is.

use crate::heap::{self, Heap, SectionHeap};
use crate::ir::IrMap;
use spacewasm::{
    AllocError, CodeBuilder, CompilerOptions, ControlFrame, DecoderStack, Engine, HostModule,
    InnerVec, JumpTarget, OperandType, WasmMemoryAllocator, WasmStream,
};
use std::alloc::Layout;
use std::cell::RefCell;
use std::fmt;
use std::ptr::NonNull;

/// Words in one IR code page; a `JumpTarget` is page in the upper 24 bits, word below.
const CODE_PAGE_WORDS: u32 = 256;

/// Limits the flight decoder was built with. Must match, or a module that loaded on board
/// may fail to replay here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderLimits {
    pub max_control_frames: usize,
    pub max_stack_depth: usize,
}

impl Default for DecoderLimits {
    /// What the C API examples pass to `spacewasm_load_module`.
    fn default() -> Self {
        DecoderLimits {
            max_control_frames: 64,
            max_stack_depth: 256,
        }
    }
}

impl DecoderLimits {
    /// Room for what a WASI toolchain emits, for a ground run.
    pub const GROUND: DecoderLimits = DecoderLimits {
        max_control_frames: 0x1_000,
        max_stack_depth: 0x400,
    };

    /// Run `f` with a `DecoderStack` of these lengths.
    pub fn with_stack<R>(&self, f: impl FnOnce(&mut DecoderStack) -> R) -> R {
        let mut control_frames = vec![ControlFrame::EMPTY; self.max_control_frames];
        let mut operands = vec![OperandType::EMPTY; self.max_stack_depth];
        f(&mut DecoderStack::new(&mut control_frames, &mut operands))
    }
}

impl fmt::Display for DecoderLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.max_control_frames, self.max_stack_depth)
    }
}

impl std::str::FromStr for DecoderLimits {
    type Err = String;

    /// `FRAMES/DEPTH`: the lengths of an embedder's `DecoderStack`, each decimal or `0x`
    /// hex.
    fn from_str(text: &str) -> Result<Self, String> {
        let number = |part: &str| {
            match part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
                Some(hex) => usize::from_str_radix(hex, 16),
                None => part.parse(),
            }
            .map_err(|err| format!("`{part}` is not a number: {err}"))
        };
        let (frames, depth) = text
            .split_once('/')
            .ok_or_else(|| format!("`{text}` is not FRAMES/DEPTH"))?;
        Ok(DecoderLimits {
            max_control_frames: number(frames)?,
            max_stack_depth: number(depth)?,
        })
    }
}

/// Why a module would not replay.
#[derive(Debug)]
pub enum ReplayError {
    /// The decoder refused it: the same module would not have loaded on board either,
    /// with these host modules and options.
    Rejected { offset: u32, reason: String },
    /// The decoder refused it as invalid code. Kept apart from `Rejected` because the
    /// cause is so often a proposal this release does not implement, which `validate`
    /// can then name.
    Decode(spacewasm::ParseError),
    /// Something the replay itself needed could not be allocated.
    Alloc(AllocError),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::Rejected { offset, reason } => write!(
                f,
                "the module does not load at byte {offset:#x}: {reason}. A module that \
                 loaded on board replays with the same host modules and options"
            ),
            ReplayError::Decode(err) => write!(
                f,
                "the module does not load at byte {:#x}: {:?}",
                err.offset, err.err.err
            ),
            ReplayError::Alloc(err) => write!(f, "out of memory replaying the load: {err:?}"),
        }
    }
}

impl std::error::Error for ReplayError {}

impl From<AllocError> for ReplayError {
    fn from(err: AllocError) -> Self {
        ReplayError::Alloc(err)
    }
}

thread_local! {
    /// Where `pc_ir_callback`, a bare `fn`, writes. One replay at a time per thread.
    static ENTRIES: RefCell<Vec<(u32, u32)>> = const { RefCell::new(Vec::new()) };
}

fn record(pc: JumpTarget, wasm_offset: usize) {
    ENTRIES.with_borrow_mut(|entries| entries.push((word(pc), wasm_offset as u32)));
}

/// A `JumpTarget` as a flat word index. Numerically the same as `JumpTarget.0`, which
/// is what a flight PC is reported as.
fn word(target: JumpTarget) -> u32 {
    target.page() as u32 * CODE_PAGE_WORDS + target.offset() as u32
}

/// Decode `module` as the flight interpreter would and record, for every compiled
/// instruction, its first IR word and the byte offset of its opcode.
///
/// PCs are relative to the module's first compiled word. On board that is word 0 only
/// if the module was the first loaded into its store; otherwise subtract where its code
/// started.
pub fn ir_map(
    module: &[u8],
    options: CompilerOptions,
    host_modules: spacewasm::Vec<HostModule>,
    limits: DecoderLimits,
) -> Result<IrMap, ReplayError> {
    let mut options = options;
    options.pc_ir_callback = Some(record);

    ENTRIES.with_borrow_mut(Vec::clear);
    let replayed = load(module, options, host_modules, limits);
    // Taken even on failure, so the next replay starts empty.
    let entries = ENTRIES.take();
    replayed?;
    Ok(IrMap::new(entries))
}

/// What a successful load cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// 16-bit IR words the module compiled to.
    pub ir_words: usize,
    /// Code pages those words took, against `CompilerOptions::max_code_pages`.
    pub code_pages: usize,
    /// What the load allocated on the global heap. `None` unless the binary's global
    /// allocator is a [`HeapRecorder`](crate::HeapRecorder).
    pub heap: Option<Heap>,
}

/// Decode and instantiate `module` as the flight interpreter would, and report what that
/// cost. Nothing runs.
pub fn load(
    module: &[u8],
    options: CompilerOptions,
    host_modules: spacewasm::Vec<HostModule>,
    limits: DecoderLimits,
) -> Result<Loaded, ReplayError> {
    let mut code = CodeBuilder::new(options)?;
    // Nothing runs, so the stack only has to exist: one word, as a zero-sized allocation
    // is refused.
    let mut engine = Engine::new(1, 1, host_modules).map_err(|err| ReplayError::Rejected {
        offset: 0,
        reason: format!("could not create the store: {err:?}"),
    })?;
    let mut stream = Bytes::new(module);
    let memory = spacewasm::Rc::new(SystemMemory)?.into_wasm_memory_allocator();

    // Everything above is the embedder's, made once per engine; from here it is the
    // module's.
    heap::start();
    let loaded = decode(&mut stream, &mut engine, &mut code, memory, limits);
    let log = heap::stop();
    loaded?;

    let code_pages = code.pages().len();
    Ok(Loaded {
        ir_words: match code_pages {
            0 => 0,
            pages => (pages - 1) * CODE_PAGE_WORDS as usize + code.offset(),
        },
        code_pages,
        heap: log.map(|log| stream.heap(log)),
    })
}

fn decode(
    stream: &mut Bytes,
    engine: &mut Engine,
    code: &mut CodeBuilder,
    memory: spacewasm::Rc<dyn WasmMemoryAllocator>,
    limits: DecoderLimits,
) -> Result<(), ReplayError> {
    // Fixed arrays on the flight side; sized to match here.
    let module = limits
        .with_stack(|stack| {
            spacewasm::Module::new("replay", stream, &mut engine.store, code, stack, memory)
        })
        .map_err(ReplayError::Decode)?;
    stream.mark(INSTANTIATE, 0);
    engine
        .push_module(module)
        .map_err(|err| ReplayError::Rejected {
            offset: 0,
            reason: format!("could not instantiate the module: {err:?}"),
        })?;
    Ok(())
}

/// The name [`Heap`] gives what is allocated after the module's last section.
const INSTANTIATE: &str = "instantiate";

/// A `WasmStream` over a module already in memory.
///
/// Every chunk ends at a section boundary. The decoder only asks for a chunk once it has
/// used every byte of the one before, so when it asks for the one that starts a section
/// it has finished the section before, and what it allocates from then on is the new
/// section's. That is where [`mark`](Self::mark) is.
struct Bytes<'a> {
    data: &'a [u8],
    pos: usize,
    /// Chunks lent to the decoder, kept alive until it hands them back.
    lent: Vec<Vec<u8>>,
    /// Where each section starts, and its name.
    sections: Vec<(usize, String)>,
    /// The next of `sections` to reach.
    next: usize,
    /// Each section reached, its size, and how many allocations had been logged when it
    /// was.
    marks: Vec<(String, usize, usize)>,
}

impl<'a> Bytes<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bytes {
            data,
            pos: 0,
            lent: Vec::new(),
            sections: heap::sections(data),
            next: 0,
            marks: Vec::new(),
        }
    }

    /// Where the `index`th section starts; the end of the module past the last.
    fn section_start(&self, index: usize) -> usize {
        self.sections.get(index).map_or(self.data.len(), |s| s.0)
    }

    fn mark(&mut self, name: &str, size: usize) {
        self.marks.push((name.to_string(), size, heap::logged()));
    }

    /// `log` split at the marks.
    fn heap(&self, log: Vec<std::alloc::Layout>) -> Heap {
        let ends = self
            .marks
            .iter()
            .skip(1)
            .map(|mark| mark.2)
            .chain([log.len()]);
        let sections = self
            .marks
            .iter()
            .zip(ends)
            .map(|((name, size, start), end)| SectionHeap {
                name: name.clone(),
                size: *size,
                allocations: log[*start..end].to_vec(),
            })
            // Nothing is allocated after the last section unless instantiating does.
            .filter(|section| section.name != INSTANTIATE || !section.allocations.is_empty())
            .collect();
        Heap { sections }
    }
}

/// Bytes handed to the decoder per read. Only affects how often it reads.
const CHUNK: usize = 1024;

impl WasmStream for Bytes<'_> {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.pos >= self.data.len() {
            return Ok(None);
        }
        while let Some((start, name)) = self.sections.get(self.next)
            && *start <= self.pos
        {
            let (name, size) = (name.clone(), self.section_start(self.next + 1) - start);
            self.mark(&name, size);
            self.next += 1;
        }
        let boundary = self.section_start(self.next);
        let end = (self.pos + CHUNK).min(self.data.len()).min(boundary);
        let mut chunk = self.data[self.pos..end].to_vec();
        self.pos = end;
        // SAFETY: `chunk` moves into `lent` below and is not touched until the decoder
        // returns this view, so the borrow stays valid and unique while it holds it.
        let view =
            unsafe { InnerVec::from_raw_parts(chunk.as_mut_ptr(), chunk.capacity(), chunk.len()) };
        self.lent.push(chunk);
        Ok(Some(view))
    }

    fn return_(&mut self, chunk: InnerVec<u8>) {
        // `InnerVec` is a borrow and frees nothing; dropping the `Vec` frees the chunk.
        let ptr = chunk.ptr().cast_const();
        self.lent.retain(|lent| lent.as_ptr() != ptr);
    }
}

/// Guest linear memory for the replay: never touched, only allocated so the module
/// instantiates as it did on board.
struct SystemMemory;

impl WasmMemoryAllocator for SystemMemory {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        if layout.size() == 0 {
            return Ok(NonNull::dangling());
        }
        // SAFETY: non-zero size, checked above.
        NonNull::new(unsafe { std::alloc::alloc_zeroed(layout) })
            .ok_or(AllocError::AllocationFailed)
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        if old_layout.size() == 0 {
            return self.allocate(layout);
        }
        // SAFETY: `ptr` came from `allocate`/`reallocate` with `old_layout`.
        NonNull::new(unsafe { std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()) })
            .ok_or(AllocError::AllocationFailed)
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 {
            // SAFETY: `ptr` came from `allocate`/`reallocate` with `layout`.
            unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
        }
    }
}
