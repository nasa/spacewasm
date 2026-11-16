// Portions of this file are derived from the Wasmtime project
// (https://github.com/bytecodealliance/wasmtime), licensed under
// Apache-2.0 WITH LLVM-exception. These portions have been modified for
// SpaceWasm.

//! Oracles.
//!
//! Oracles take a test case and determine whether we have a bug. For example,
//! one of the simplest oracles is to take a Wasm binary as our input test case,
//! validate and instantiate it, and (implicitly) check that no assertions
//! failed or segfaults happened.
//!
//! When an oracle finds a bug, it should report it to the fuzzing engine by
//! panicking.

use spacewasm::*;
use std::alloc::Layout;
use std::ptr::NonNull;

static ORACLE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Log a Wasm module to the filesystem for debugging.
///
/// This is only enabled when `RUST_LOG=debug` is set.
pub fn log_wasm(wasm: &[u8]) {
    crate::init_fuzzing();

    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    let i = ORACLE_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = format!("testcase{i}.wasm");
    std::fs::write(&name, wasm).ok();
    log::debug!("wrote wasm file to `{name}`");

    let wat = format!("testcase{i}.wat");
    match wasmprinter::print_bytes(wasm) {
        Ok(s) => {
            std::fs::write(&wat, s).ok();
            log::debug!("wrote wat file to `{wat}`");
        }
        Err(e) => {
            log::debug!("failed to print to wat: {e}");
            std::fs::remove_file(&wat).ok();
        }
    }
}

/// A simple byte stream implementation for fuzzing.
pub(crate) struct ByteStream {
    buffer: Option<std::vec::Vec<u8>>,
    consumed: bool,
}

impl ByteStream {
    pub(crate) fn new(data: &[u8]) -> Self {
        Self {
            buffer: Some(data.to_vec()),
            consumed: false,
        }
    }
}

impl WasmStream for ByteStream {
    fn read(&mut self) -> Result<Option<InnerVec<u8>>, u8> {
        if self.consumed {
            return Ok(None);
        }

        if let Some(ref mut vec) = self.buffer {
            self.consumed = true;
            let inner = unsafe { InnerVec::from_raw_parts(vec.as_mut_ptr(), vec.len(), vec.len()) };
            Ok(Some(inner))
        } else {
            Ok(None)
        }
    }

    fn return_(&mut self, _chunk: InnerVec<u8>) {
        // Buffer is kept alive in self.buffer, so nothing to do
    }
}

/// Largest single allocation either fuzz allocator will service, in bytes.
///
/// A malformed module can decode a bogus length prefix -- a function-section
/// count, a memory or table size, and so on -- into an enormous single
/// allocation request (a `0x0FFFFFFF` function count drives a ~8.6 GiB `Vec`,
/// for example). Handing that straight to the system allocator makes the
/// process OOM/abort, which libFuzzer reports as a crash even though it is just
/// an unreasonable request, not a bug in the decoder. Bounding each request to
/// a fixed cap turns that into a clean `AllocError::OutOfMemory`, which the
/// decoder already converts into a `ValidationError`, so the fuzzer keeps
/// hunting for real defects instead of tripping over absurd sizes.
///
/// This models a real embedding, which always drives the decoder through a
/// bounded allocator -- fixed, measured memory is the whole point of running
/// Wasm on-board. Cumulative growth across many allocations is left to
/// libFuzzer's own `-rss_limit_mb` guard.
const MAX_ALLOCATION_BYTES: usize = 1024 * 1024 * 64; // 64 MiB

/// The global allocator for fuzzing, backing the decoder's internal bookkeeping
/// (module tables, `Vec`s of parsed entries, ...) via `global_allocator!`.
///
/// A thin passthrough to the system allocator whose only policy is the shared
/// per-allocation cap ([`MAX_ALLOCATION_BYTES`]).
pub(crate) struct SystemAllocator;

unsafe impl Allocator for SystemAllocator {
    unsafe fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        if layout.size() > MAX_ALLOCATION_BYTES {
            return Err(AllocError::OutOfMemory);
        }
        let ptr = unsafe { std::alloc::alloc(layout) };
        if ptr.is_null() {
            return Err(AllocError::AllocationFailed);
        }
        Ok(ptr)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

/// Size of each worker's guest linear-memory pool (see [`FuzzAllocator`]).
const GUEST_POOL_BYTES: usize = MAX_ALLOCATION_BYTES;

/// Alignment of the guest pool base; covers every `MemPageSize::alignment`.
const GUEST_POOL_ALIGN: usize = 16;

thread_local! {
    /// The guest pool, allocated once and reused by every run on this thread.
    /// libFuzzer workers are separate processes, so this is one fixed pool per
    /// worker.
    static GUEST_POOL: NonNull<u8> = {
        let layout = Layout::from_size_align(GUEST_POOL_BYTES, GUEST_POOL_ALIGN).unwrap();
        NonNull::new(unsafe { std::alloc::alloc(layout) }).expect("guest pool allocation failed")
    };

    /// Whether a [`FuzzAllocator`] currently owns [`GUEST_POOL`].
    static GUEST_POOL_IN_USE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A bump allocator over a fixed per-worker pool ([`GUEST_POOL`]).
pub(crate) struct FuzzAllocator {
    base: NonNull<u8>,
    offset: std::cell::Cell<usize>,
}

impl FuzzAllocator {
    /// Take ownership of this thread's guest pool, rewinding it.
    pub(crate) fn new() -> Self {
        assert!(
            !GUEST_POOL_IN_USE.replace(true),
            "guest pool already owned by another FuzzAllocator"
        );
        Self {
            base: GUEST_POOL.with(|p| *p),
            offset: std::cell::Cell::new(0),
        }
    }
}

impl Drop for FuzzAllocator {
    fn drop(&mut self) {
        GUEST_POOL_IN_USE.set(false);
    }
}

impl WasmMemoryAllocator for FuzzAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        if layout.align() > GUEST_POOL_ALIGN {
            return Err(AllocError::AllocationFailed);
        }
        let start = self.offset.get().next_multiple_of(layout.align());
        if start > GUEST_POOL_BYTES || layout.size() > GUEST_POOL_BYTES - start {
            return Err(AllocError::OutOfMemory);
        }
        self.offset.set(start + layout.size());
        Ok(unsafe { self.base.add(start) })
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        let start = unsafe { ptr.offset_from(self.base) } as usize;
        // Only the last allocation can grow, and only in place.
        if start + old_layout.size() != self.offset.get() {
            return Err(AllocError::OutOfMemory);
        }
        if layout.size() > GUEST_POOL_BYTES - start {
            return Err(AllocError::OutOfMemory);
        }
        self.offset.set(start + layout.size());
        Ok(ptr)
    }

    fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {}
}

// Set up the global allocator for fuzzing
#[allow(missing_docs)]
mod fuzz_alloc {
    use super::SystemAllocator;
    spacewasm::global_allocator!(SystemAllocator, SystemAllocator);
}

const MAX_CODE_PAGES: usize = 128;
const MAX_CONTROL_FRAMES: usize = 128;
const MAX_STACK_DEPTH: usize = 256;

/// Decode and validate a Wasm module, returning SpaceWasm's classified outcome.
///
/// `Ok(())` means SpaceWasm accepted the module; `Err(validation_error)` is a
/// graceful rejection carrying the concrete [`ValidationError`] so callers can
/// classify *why* SpaceWasm refused it (see [`is_benign_rejection`]). Neither is a
/// bug on its own. The only failures this surfaces implicitly are a `panic!`
/// (including the `strict-assertions` checks), an abort, a segfault, or a
/// sanitizer trip during `Module::new`; libFuzzer treats those as crashes.
///
/// Store and `CodeBuilder` setup use fixed sizes independent of the fuzz input,
/// so a failure there is an infrastructure problem, not a module rejection --
/// hence the `.expect(...)`s rather than a spurious `Err`.
fn validate_module(wasm: &[u8]) -> Result<(), ValidationError> {
    let mut store =
        Store::from_host_modules(256, Vec::zero()).expect("fuzz store creation must not fail");

    let mut code_builder =
        CodeBuilder::new(CompilerOptions::new(true, None, MAX_CODE_PAGES)).unwrap();
    let mut stream = ByteStream::new(wasm);

    let allocator = spacewasm::Rc::new(FuzzAllocator::new())
        .unwrap()
        .into_wasm_memory_allocator();

    // Attempt to decode and validate the module. `ParseError` wraps a
    // `SectionDecodeError`, which in turn carries the innermost `ValidationError`.
    let mut control_frames = [ControlFrame::EMPTY; MAX_CONTROL_FRAMES];
    let mut operands = [OperandType::EMPTY; MAX_STACK_DEPTH];
    let mut decoder_stack = DecoderStack::new(&mut control_frames, &mut operands);
    Module::new(
        "",
        &mut stream,
        &mut store,
        &mut code_builder,
        &mut decoder_stack,
        allocator.clone(),
    )
    .map(|_| ())
    .map_err(|e: ParseError| e.err.err)
}

/// Decode and validate a Wasm module, discarding the outcome.
///
/// Returns `true` if decoding succeeded, for callers that only need the
/// accept/reject bit (see [`validate_module`] for the classified variant).
fn load_module(wasm: &[u8]) -> bool {
    validate_module(wasm).is_ok()
}

/// Rejections that are *not* evidence of a SpaceWasm validation-completeness bug,
/// even though wasmi's validator accepted the same module. The completeness
/// direction of [`validate_differential`] must not treat these as divergences.
/// They fall into four groups:
///
/// **Embedded resource limits.** SpaceWasm bounds what a general validator does
/// not. The fuzzer's configured limits funnel here: the code-page, control-frame,
/// and operand-stack bounds surface as `AllocError`; oversized decoded vectors as
/// `VecTooLong`; over-range branch offsets as `LabelJumpTooLarge`; the 16-bit
/// local cap as `TooManyLocals`; a table larger than `MAX_TABLE_ELEMENTS`
/// (10M, well under the spec's 2^32-1) as `TableTooLarge`; and a function with
/// more than 255 parameter slots as `FunctionParametersTooLarge`.
///
/// **Instantiation-time checks.** SpaceWasm's `Module::new` decodes, validates,
/// *and* instantiates in one pass (it allocates guest memory and eagerly
/// initializes tables and data), whereas `wasmi::Module::validate` only validates.
/// Conditions the Wasm MVP spec defers to instantiation therefore make SpaceWasm
/// reject at load while wasmi's validator accepts: guest-memory allocation
/// (`GuestMemoryAllocationFailure`, bounded to 64 MiB here), out-of-bounds active
/// element segments (`InvalidElementOffset`, `InvalidElementOutOfBounds`), and
/// out-of-bounds active data segments (`MemoryError` from the eager memory write).
///
/// **wasmi feature-gating leniency.** wasmi -- even with the bulk-memory proposal
/// disabled -- accepts encodings SpaceWasm's strict MVP decoder rejects: a
/// `DataCount` section (id 12) surfaces as `MalformedSectionId(12)`, and a data
/// segment using the bulk-memory flag byte (`0x01` passive / `0x02` explicit
/// memidx) is read by SpaceWasm as a nonzero memory index, surfacing as
/// `InvalidMemIndex`. wasmi does not gate these on the feature flag, and there is
/// no wasmi config knob to match.
///
/// **Spec-version differences.** wasmi implements the Wasm 2.0 validation rules,
/// which relax some checks relative to the Wasm 1.0 rules SpaceWasm targets
/// (REQ-13). The one that surfaces here is `br_table`: Wasm 1.0 requires every
/// target label to have the *same* result type, whereas 2.0 lets the target types
/// differ (each need only match its label under subtyping). A `br_table` in
/// unreachable code whose targets share arity but differ in type is therefore
/// valid under 2.0 (wasmi accepts) and invalid under 1.0 (SpaceWasm rejects with
/// `BrTableResultTypeMismatch`).
/// See <https://github.com/nasa/spacewasm/issues/171#issuecomment-5333770458>.
#[cfg(feature = "differential")]
fn is_benign_rejection(err: &ValidationError) -> bool {
    matches!(
        err,
        // Embedded resource limits.
        ValidationError::AllocError(_)
            | ValidationError::VecTooLong
            | ValidationError::LabelJumpTooLarge
            | ValidationError::TooManyLocals
            | ValidationError::TableTooLarge
            | ValidationError::FunctionParametersTooLarge
            // Instantiation-time checks (deferred past validation by the spec).
            | ValidationError::MemoryError(_)
            | ValidationError::GuestMemoryAllocationFailure
            | ValidationError::InvalidElementOffset
            | ValidationError::InvalidElementOutOfBounds
            // wasmi feature-gating leniency (bulk-memory encodings it accepts
            // even with the proposal disabled).
            | ValidationError::MalformedSectionId(12)
            | ValidationError::InvalidMemIndex
            // Spec-version difference: a `br_table` whose targets share arity but
            // differ in type is valid under wasm 2.0 (wasmi) and invalid under wasm
            // 1.0 (SpaceWasm).
            | ValidationError::BrTableResultTypeMismatch
    )
}

/// Oracle: Validate a Wasm module.
///
/// This tests the decoder by attempting to validate the given Wasm bytes.
/// It checks that the module is structurally valid according to Wasm spec.
///
/// Inputs come from wasm-smith and are therefore expected to be valid, so a
/// decode error here points at SpaceWasm being stricter than the generator
/// (e.g. code-page or stack limits) rather than at malformed input.
pub fn validate(wasm: &[u8]) {
    log_wasm(wasm);

    if load_module(wasm) {
        log::debug!("validation succeeded");
    } else {
        log::debug!("validation failed (expected for invalid modules)");
    }
}

/// Oracle: Decode a possibly-malformed Wasm module.
///
/// This tests the decoder against intentionally corrupted input (see
/// [`crate::generators::MalformedModule`]). A clean `Err` is the correct
/// outcome for garbage input; the bug we are hunting is a *reachable panic* --
/// an out-of-bounds read, an unchecked index, an arithmetic overflow, or a
/// `strict-assertions` violation -- on the load path. The decoder must reject
/// bad modules gracefully, never crash on them.
pub fn decode(wasm: &[u8]) {
    log_wasm(wasm);

    if load_module(wasm) {
        log::debug!("decode succeeded (mutation happened to stay valid)");
    } else {
        log::debug!("decode rejected malformed module (expected)");
    }
}

/// Oracle: Execute module that should not trap.
///
/// This tests modules generated with disallow_traps configuration.
/// Such modules should never trap during execution - if they do, it's a bug
/// in either the generator or the interpreter.
///
/// This oracle uses execution tracing to record pc/sp/fp history,
/// which is dumped on panic for debugging.
pub fn no_traps(wasm: &[u8]) {
    log_wasm(wasm);

    // Create engine with reduced store size for better parallel fuzzing
    let mut state = match Engine::new(512, 16, Vec::zero()) {
        Ok(s) => s,
        Err(e) => {
            log::debug!("engine creation failed: {e:?}");
            return;
        }
    };

    // Compile module with reduced code pages
    let mut code_builder =
        CodeBuilder::new(CompilerOptions::new(true, None, MAX_CODE_PAGES)).unwrap();
    let mut stream = ByteStream::new(wasm);

    let allocator = Rc::new(FuzzAllocator::new())
        .unwrap()
        .into_wasm_memory_allocator();

    let mut control_frames = [ControlFrame::EMPTY; MAX_CONTROL_FRAMES];
    let mut operands = [OperandType::EMPTY; MAX_STACK_DEPTH];
    let mut decoder_stack = DecoderStack::new(&mut control_frames, &mut operands);
    let module = match Module::new(
        "",
        &mut stream,
        &mut state.store,
        &mut code_builder,
        &mut decoder_stack,
        allocator.clone(),
    ) {
        Ok(m) => m,
        Err(e) => {
            log::debug!("compilation failed: {e:?}");
            return;
        }
    };

    log::debug!("module compiled successfully");

    // Borrow the compiled text straight from the builder (no copy needed).
    let text = code_builder.pages();

    let module_ref = state.push_module(module).unwrap();
    let start_result = match state.module_start(module_ref) {
        None => InterpreterResult::Finished,
        Some(start) => match state.invoke(start, &[]) {
            Ok(()) => Interpreter.run(text, &mut state, 10000),
            Err(InvokeError::StackOverflow) => InterpreterResult::Trap(TrapReason::StackOverflow),
            // The start function is validated to be `[] -> []`, so parameter
            // length/type mismatches cannot occur.
            Err(_) => unreachable!(),
        },
    };
    match start_result {
        InterpreterResult::Finished => {}
        InterpreterResult::OutOfFuel => {
            log::debug!("start routine out of fuel");
            return;
        }
        InterpreterResult::Trap(TrapReason::StackOverflow) => {
            // Wasm Smith cannot avoid this. Also this is not a bug so it's ok to drop this run
            log::debug!("module hit a stack overflow during initialization");
            return;
        }
        InterpreterResult::Trap(trap_reason) => {
            panic!("Trap during initialization: {trap_reason:?}")
        }
        InterpreterResult::Pause => panic!("Host init pause"),
    }

    log::debug!("module instantiated");

    // Get the last module index and collect exported function refs with their signatures
    let module_idx = state.store.modules().len().saturating_sub(1);

    let exported_funcs: std::vec::Vec<(WasmRef, std::vec::Vec<Value>)> = {
        let Some(module) = state.store.modules().get(module_idx) else {
            log::debug!("failed to get module");
            return;
        };

        module
            .exports
            .iter()
            .filter_map(|export| {
                if let ExportDesc::Func(func_idx) = export.desc {
                    // Get the function reference which handles import resolution
                    let func_ref = module.get_func_ref(func_idx)?;

                    // Look up the function type based on the resolved reference
                    let func_type = match func_ref {
                        Ref::Module(index) => {
                            // Local function in this module
                            let func = module.functions.get(index as usize)?;
                            module.types.get(func.ty.0 as usize)?
                        }
                        Ref::Extern {
                            module: mod_ref,
                            index,
                        } => {
                            // Function from another Wasm module
                            let other_module = state.store.modules().get(mod_ref.0 as usize)?;
                            let func = other_module.functions.get(index as usize)?;
                            other_module.types.get(func.ty.0 as usize)?
                        }
                        Ref::Host { .. } => {
                            // Host function - skip these for now since they have different handling
                            return None;
                        }
                    };

                    // Convert func_ref to WasmRef
                    let wasm_ref = match func_ref {
                        Ref::Module(index) => WasmRef {
                            module: ModuleRef(module_idx as u8),
                            index,
                        },
                        Ref::Extern { module, index } => WasmRef { module, index },
                        _ => return None,
                    };

                    // Generate default parameters based on the function signature
                    let params: std::vec::Vec<Value> = func_type
                        .params
                        .iter()
                        .map(|val_type| match val_type {
                            ValType::I32 => Value::I32(0),
                            ValType::I64 => Value::I64(0),
                            ValType::F32 => Value::F32(0.0),
                            ValType::F64 => Value::F64(0.0),
                        })
                        .collect();

                    Some((wasm_ref, params))
                } else {
                    None
                }
            })
            .collect()
    };

    // Try to invoke each exported function
    // These should never trap since the module was generated with disallow_traps
    for (wasm_ref, params) in exported_funcs {
        state.reset();
        match state.invoke(wasm_ref, &params) {
            Ok(()) => {}
            // Wasm Smith cannot avoid deeply-recursive exports; a stack overflow
            // here is not a bug, so skip this export.
            Err(InvokeError::StackOverflow) => {
                log::debug!("export hit a stack overflow during invocation");
                continue;
            }
            Err(e) => panic!("unexpected invoke error in no_traps module: {e:?}"),
        }

        // Run the interpreter with limited instructions
        let interpreter = Interpreter;
        let result = interpreter.run(text, &mut state, 10000);

        // Check for traps - this is the key assertion
        match result {
            InterpreterResult::OutOfFuel => {
                log::debug!("ran out of fuel (instruction limit reached)");
            }
            InterpreterResult::Finished => {
                log::debug!("execution completed without traps");
            }
            InterpreterResult::Trap(TrapReason::StackOverflow) => {
                log::debug!("execution completed with stack overflow");
            }
            InterpreterResult::Trap(reason) => {
                // A trap in a no_traps module is a bug!
                panic!("unexpected trap in no_traps module: {reason:?}");
            }
            InterpreterResult::Pause => {
                panic!("interpreter paused by host function")
            }
        }
    }
}

/// Build the wasmi `Engine` used for *validation* differential testing.
///
/// Matched to SpaceWasm's accepted language: Wasm 1.0 MVP **plus** the post-MVP
/// proposals SpaceWasm supports (custom page sizes, sign-extension operators, and
/// non-trapping float-to-int conversions). Feature parity matters
/// here -- a feature one validator enables and the other doesn't would make them
/// disagree for reasons that are not bugs. Fuel is irrelevant to validation.
#[cfg(feature = "differential")]
fn build_wasmi_validate_engine() -> wasmi::Engine {
    wasmi::Engine::new(&wasmi_config())
}

/// The wasmi `Config` matched to SpaceWasm's accepted language, shared by the
/// validation and execution differential engines.
#[cfg(feature = "differential")]
fn wasmi_config() -> wasmi::Config {
    let mut config = wasmi::Config::default();
    config
        .wasm_mutable_global(true)
        .floats(true)
        // SpaceWasm supports custom page sizes; enable it so the two validators
        // agree on modules that use them.
        .wasm_custom_page_sizes(true)
        .wasm_sign_extension(true)
        .wasm_saturating_float_to_int(true)
        // Everything else post-MVP: off.
        .wasm_multi_value(false)
        .wasm_multi_memory(false)
        .wasm_bulk_memory(false)
        .wasm_reference_types(false)
        .wasm_tail_call(false)
        .wasm_extended_const(false)
        .wasm_memory64(false)
        .wasm_wide_arithmetic(false);
    config
}

#[cfg(feature = "differential")]
fn with_wasmi_validate_engine<R>(f: impl FnOnce(&wasmi::Engine) -> R) -> R {
    thread_local! {
        static ENGINE: wasmi::Engine = build_wasmi_validate_engine();
    }
    ENGINE.with(f)
}

/// Oracle: differentially test the *validator*.
///
/// Decodes+validates the same bytes with both engines and checks the accept /
/// reject decisions against wasmi (a conformant validator with a matched feature
/// set). Both directions are asserted:
///
/// * **Soundness** -- SpaceWasm *accepts* what wasmi *rejects*: a module SpaceWasm
///   should have refused. Reported by panicking.
/// * **Completeness** -- SpaceWasm *rejects* what wasmi *accepts*: a module
///   SpaceWasm should have admitted. Reported by panicking, *unless* the rejection
///   is not a Wasm-validation failure ([`is_benign_rejection`]) -- an
///   embedded resource limit, or an instantiation-time check that SpaceWasm's
///   combined decode+validate+instantiate pass performs at load but the spec (and
///   thus wasmi's validator) defers. Those rejections are legitimate and filtered
///   out rather than flagged.
///
/// Best driven with the [`crate::generators::MalformedModule`] generator, whose
/// byte-level mutations of valid modules densely exercise the accept/reject
/// boundary.
#[cfg(feature = "differential")]
pub fn validate_differential(wasm: &[u8]) {
    crate::init_fuzzing();

    // `validate_module` runs SpaceWasm's full decode + validate and reports
    // whether it accepted the module, and if not, the concrete rejection reason.
    let space_result = validate_module(wasm);
    // Use `Module::validate`, not `Module::new`: the latter compiles lazily by
    // default and defers (or skips) validation for inputs it never fully parses,
    // so it leniently reports `Ok` on truncated/headerless garbage that is not
    // actually valid. `validate` eagerly validates the whole module, matching
    // what SpaceWasm's decoder does.
    let wasmi_ok =
        with_wasmi_validate_engine(|engine| wasmi::Module::validate(engine, wasm).is_ok());

    match space_result {
        // Soundness: SpaceWasm accepted a module wasmi rejects as invalid.
        Ok(()) if !wasmi_ok => {
            log_wasm(wasm);
            panic!(
                "validation soundness divergence: SpaceWasm accepted a module that wasmi rejected as invalid"
            );
        }
        // Completeness: SpaceWasm rejected -- for a genuine validation reason, not
        // a resource limit or instantiation-time check -- a module wasmi accepts.
        Err(err) if wasmi_ok && !is_benign_rejection(&err) => {
            log_wasm(wasm);
            panic!(
                "validation completeness divergence: SpaceWasm rejected ({err:?}) a module that wasmi accepted as valid"
            );
        }
        // Agreement, or an allow-listed resource-limit rejection: not a bug.
        _ => {}
    }
}

#[cfg(all(test, feature = "differential"))]
mod validate_differential_tests {
    use super::*;

    /// The validation oracle must not fire on modules the two validators agree
    /// on: a well-formed MVP module (both accept) and garbage (both reject).
    #[test]
    fn validate_agrees_on_valid_and_garbage() {
        let valid =
            wat::parse_str(r#"(module (func (export "f") (result i32) i32.const 1))"#).unwrap();
        // Both accept -> no panic.
        validate_differential(&valid);
        assert!(load_module(&valid));
        assert!(with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &valid
        )
        .is_ok()));

        // Random garbage: both reject -> no panic, and neither validator accepts.
        let garbage = [0x00u8, 0x61, 0x73, 0x6d, 0xff, 0xff, 0xff, 0xff, 0x13, 0x37];
        validate_differential(&garbage);
        assert!(!with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &garbage
        )
        .is_ok()));

        // A module using a disabled proposal (multi-value result) must be
        // rejected by both, so the oracle stays quiet.
        let multi = wat::parse_str(
            r#"(module (func (export "f") (result i32 i32) i32.const 1 i32.const 2))"#,
        )
        .unwrap();
        validate_differential(&multi);
        assert!(!load_module(&multi));
        assert!(!with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &multi
        )
        .is_ok()));
    }

    /// A spec-valid module that trips one of SpaceWasm's embedded resource limits
    /// must be filtered out of the completeness direction, not reported as a
    /// divergence. Here the operand stack is grown past `MAX_STACK_DEPTH` (256)
    /// with a run of `i32.const`s that are then dropped, so the function is valid
    /// per spec (wasmi accepts) but SpaceWasm rejects with an `AllocError` when
    /// its fixed-size operand stack overflows.
    #[test]
    fn validate_ignores_resource_limit_rejections() {
        // Push more values than the operand-stack bound, then drop them all so the
        // function's result type is empty and the module stays spec-valid.
        let depth = MAX_STACK_DEPTH + 64;
        let mut body = String::new();
        for _ in 0..depth {
            body.push_str("i32.const 0 ");
        }
        for _ in 0..depth {
            body.push_str("drop ");
        }
        let wasm = wat::parse_str(format!(r#"(module (func (export "f") {body}))"#)).unwrap();

        // SpaceWasm rejects, and the rejection is an allow-listed resource limit.
        let err = validate_module(&wasm).expect_err("operand stack must overflow the limit");
        assert!(
            is_benign_rejection(&err),
            "expected a resource-limit rejection, got {err:?}"
        );

        // wasmi accepts the same module as valid.
        assert!(with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &wasm
        )
        .is_ok()));

        // The two therefore disagree, but the oracle must stay quiet (no panic).
        validate_differential(&wasm);
    }

    /// Regression: a short, header-less blob is invalid and both validators must
    /// reject it. wasmi's lazily-compiling `Module::new` leniently reports `Ok`
    /// here (it never fully parses the input), which would spuriously trip the
    /// completeness direction; `Module::validate` -- what the oracle uses -- does
    /// not. This case was found by the fuzzer.
    #[test]
    fn validate_rejects_headerless_blob_like_wasmi_validate() {
        let blob = [10u8, 10, 40, 64, 37, 41];

        // SpaceWasm rejects (no `\0asm` magic).
        assert_eq!(validate_module(&blob), Err(ValidationError::MalformedMagic));

        // wasmi's eager validator also rejects it...
        assert!(!with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &blob
        )
        .is_ok()));
        // ...even though its lazy `Module::new` leniently accepts it.
        assert!(with_wasmi_validate_engine(
            |e| wasmi::Module::new(e, blob).is_ok()
        ));

        // Both reject -> the oracle stays quiet.
        validate_differential(&blob);
    }

    /// Regression: an active element segment whose offset lies past the table's
    /// size is *valid* per the Wasm MVP spec (it fails at instantiation, which a
    /// pure validator like wasmi does not perform), but SpaceWasm's combined
    /// decode+validate+instantiate pass rejects it eagerly at load. That
    /// instantiation-time rejection must be filtered out of the completeness
    /// direction rather than reported as a divergence. This class of divergence
    /// was found by the fuzzer.
    #[test]
    fn validate_ignores_instantiation_time_rejections() {
        // Table holds 1 element; the element segment targets offset 5 -> out of
        // bounds at instantiation, but structurally valid.
        let wasm =
            wat::parse_str(r#"(module (func) (table 1 funcref) (elem (i32.const 5) 0))"#).unwrap();

        // SpaceWasm rejects with an instantiation-time check that is filtered out.
        let err = validate_module(&wasm).expect_err("element segment is out of bounds");
        assert_eq!(err, ValidationError::InvalidElementOffset);
        assert!(is_benign_rejection(&err));

        // wasmi's validator accepts it (the spec defers the bounds check).
        assert!(with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &wasm
        )
        .is_ok()));

        // They disagree, but the oracle must stay quiet (no panic).
        validate_differential(&wasm);
    }

    /// Regression: a `DataCount` section (id 12, from the bulk-memory proposal) is
    /// not part of the Wasm MVP, so SpaceWasm rejects it as `MalformedSectionId(12)`.
    /// wasmi accepts it even with bulk memory disabled -- its parser does not gate
    /// the section's presence -- so this feature-gating leniency must be filtered
    /// out of the completeness direction. This case was found by the fuzzer.
    #[test]
    fn validate_ignores_wasmi_datacount_leniency() {
        // A minimal MVP module (one `() -> ()` function) with a `DataCount 0`
        // section spliced in before the code section.
        let wasm = [
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
            0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type: () -> ()
            0x03, 0x02, 0x01, 0x00, // function: 1 func, type 0
            0x0c, 0x01, 0x00, // DataCount section (id 12), value 0
            0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b, // code: 1 func, empty body
        ];

        // SpaceWasm rejects the post-MVP section; the rejection is filtered out.
        let err = validate_module(&wasm).expect_err("DataCount is not an MVP section");
        assert_eq!(err, ValidationError::MalformedSectionId(12));
        assert!(is_benign_rejection(&err));

        // wasmi accepts it despite `wasm_bulk_memory(false)`.
        assert!(with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &wasm
        )
        .is_ok()));

        // They disagree, but the oracle must stay quiet (no panic).
        validate_differential(&wasm);
    }

    /// Regression: a table larger than `MAX_TABLE_ELEMENTS` (10M) is within the
    /// Wasm spec's 2^32-1 limit, so wasmi accepts it, but exceeds SpaceWasm's
    /// embedded cap. That resource-limit rejection must be filtered out of the
    /// completeness direction. This case was found by the fuzzer.
    #[test]
    fn validate_ignores_oversized_table_limit() {
        let wasm = wat::parse_str(r#"(module (table 16400383 funcref))"#).unwrap();

        let err = validate_module(&wasm).expect_err("table exceeds the embedded cap");
        assert_eq!(err, ValidationError::TableTooLarge);
        assert!(is_benign_rejection(&err));

        assert!(with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &wasm
        )
        .is_ok()));

        validate_differential(&wasm);
    }

    /// Regression: a function type with more than 255 parameter slots is valid
    /// per the Wasm spec (wasmi accepts it), but exceeds SpaceWasm's embedded
    /// parameter cap and is rejected as `FunctionParametersTooLarge`. That
    /// resource-limit rejection must be filtered out of the completeness
    /// direction rather than reported as a divergence.
    #[test]
    fn validate_ignores_oversized_function_params() {
        // 300 i32 params = 300 four-byte slots, past the 255-slot cap.
        let params = "i32 ".repeat(300);
        let wasm = wat::parse_str(format!(r#"(module (func (param {params})))"#)).unwrap();

        let err = validate_module(&wasm).expect_err("param count exceeds the embedded cap");
        assert_eq!(err, ValidationError::FunctionParametersTooLarge);
        assert!(is_benign_rejection(&err));

        assert!(with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &wasm
        )
        .is_ok()));

        validate_differential(&wasm);
    }

    /// Regression: a data segment encoded with the bulk-memory flag byte `0x02`
    /// (active, explicit memidx) is accepted by wasmi even with bulk memory
    /// disabled, but SpaceWasm's MVP decoder reads the flag as the memory index
    /// and rejects it as `InvalidMemIndex`. That wasmi feature-gating leniency
    /// must be filtered out of the completeness direction. Built as raw bytes
    /// because `wat` will not emit the bulk-memory encoding for an MVP module.
    /// This case was found by the fuzzer.
    #[test]
    fn validate_ignores_wasmi_bulk_data_flag_leniency() {
        let wasm = [
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
            0x05, 0x03, 0x01, 0x00, 0x01, // memory: 1 memory, min 1 page
            // data section: 1 segment, flag 0x02 (active + explicit memidx),
            // memidx 0, offset (i32.const 0) (end), 0 bytes.
            0x0b, 0x07, 0x01, 0x02, 0x00, 0x41, 0x00, 0x0b, 0x00,
        ];

        let err = validate_module(&wasm).expect_err("bulk-memory data flag is not MVP");
        assert_eq!(err, ValidationError::InvalidMemIndex);
        assert!(is_benign_rejection(&err));

        assert!(with_wasmi_validate_engine(|e| wasmi::Module::validate(
            e, &wasm
        )
        .is_ok()));

        validate_differential(&wasm);
    }
}

/// Instructions (SpaceWasm) or fuel units (wasmi) each exported call may run.
///
/// The two engines count differently -- SpaceWasm counts IR instructions and
/// wasmi charges fuel per instruction group -- so a call that exhausts its budget
/// in either engine is inconclusive and its results are not compared. The budget
/// only needs to be large enough that most generated calls finish in both.
#[cfg(feature = "differential")]
const EXEC_FUEL: u64 = 100_000;

/// Operand stack size (in 32-bit words) of the SpaceWasm engine.
#[cfg(feature = "differential")]
const EXEC_STACK_WORDS: usize = 16 * 1024;

/// The observable outcome of running a module's start function or one exported
/// call in one engine.
#[cfg(feature = "differential")]
#[derive(Debug, Clone, Copy, PartialEq)]
enum ExecOutcome {
    /// Returned normally, with the function's result (if any).
    Returned(Option<Value>),
    /// Trapped, classified into a [`Trap`] both engines share.
    Trap(Trap),
    /// Ran out of fuel or stack. Engine limits differ, so this is not compared.
    Exhausted,
}

/// A trap kind both engines report, at the granularity the spec defines.
///
/// SpaceWasm distinguishes some traps more finely than wasmi does (or vice
/// versa); this collapses both into the spec's trap categories so equal traps
/// compare equal.
#[cfg(feature = "differential")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trap {
    Unreachable,
    MemoryOutOfBounds,
    /// `call_indirect` index past the end of the table.
    TableOutOfBounds,
    /// `call_indirect` to a table slot that holds no function.
    UninitializedElement,
    /// `call_indirect` whose callee's type does not match the expected type.
    IndirectCallTypeMismatch,
    DivideByZero,
    IntegerOverflow,
    BadConversionToInteger,
}

#[cfg(feature = "differential")]
impl Trap {
    /// Classify a SpaceWasm trap. Returns `None` for traps with no wasmi
    /// counterpart, which the generated modules (no imports) must never hit.
    fn from_spacewasm(reason: TrapReason) -> Option<Self> {
        Some(match reason {
            TrapReason::Unreachable => Self::Unreachable,
            TrapReason::MemoryOutOfBounds => Self::MemoryOutOfBounds,
            TrapReason::InvalidTableIndex => Self::TableOutOfBounds,
            TrapReason::UninitializedTableElement => Self::UninitializedElement,
            TrapReason::InvalidTableFunctionType => Self::IndirectCallTypeMismatch,
            TrapReason::DivideByZero => Self::DivideByZero,
            // Signed division overflow and an out-of-range float-to-int
            // truncation are both "integer overflow" in the spec.
            TrapReason::IntegerOverflow | TrapReason::UnrepresentableResult => {
                Self::IntegerOverflow
            }
            TrapReason::BadConversionToInteger => Self::BadConversionToInteger,
            _ => return None,
        })
    }

    /// Classify a wasmi trap. Returns `None` for codes with no SpaceWasm
    /// counterpart in this setup.
    fn from_wasmi(code: wasmi::TrapCode) -> Option<Self> {
        use wasmi::TrapCode;
        Some(match code {
            TrapCode::UnreachableCodeReached => Self::Unreachable,
            TrapCode::MemoryOutOfBounds => Self::MemoryOutOfBounds,
            TrapCode::TableOutOfBounds => Self::TableOutOfBounds,
            TrapCode::IndirectCallToNull => Self::UninitializedElement,
            TrapCode::BadSignature => Self::IndirectCallTypeMismatch,
            TrapCode::IntegerDivisionByZero => Self::DivideByZero,
            TrapCode::IntegerOverflow => Self::IntegerOverflow,
            TrapCode::BadConversionToInteger => Self::BadConversionToInteger,
            _ => return None,
        })
    }
}

/// Module state visible after a call: every exported global's value and the
/// contents of the exported memory. Compared after each call, so a divergence
/// in a store or `global.set` is caught even when the return value agrees.
#[cfg(feature = "differential")]
#[derive(Debug, PartialEq)]
struct ExecState {
    globals: std::vec::Vec<(std::string::String, Value)>,
    memory: Option<std::vec::Vec<u8>>,
}

/// An exported function's signature. Results are at most one value, since
/// multi-value is disabled.
#[cfg(feature = "differential")]
struct ExecSig {
    params: std::vec::Vec<ValType>,
    ret: Option<ValType>,
}

/// One exported function to call, with its arguments.
#[cfg(feature = "differential")]
struct ExecCall {
    name: std::string::String,
    params: std::vec::Vec<Value>,
}

/// Derive a deterministic argument list for `params` from the fuzz input.
///
/// Mixes in boundary values (0, -1, `MIN`, `MAX`, NaN, infinities) because those
/// are where trapping arithmetic and conversions diverge.
#[cfg(feature = "differential")]
fn exec_args(u: &mut arbitrary::Unstructured<'_>, params: &[ValType]) -> std::vec::Vec<Value> {
    params
        .iter()
        .map(|ty| {
            // On exhausted input `arbitrary` yields zeros, which is a fine default.
            let pick = u.int_in_range(0..=7u8).unwrap_or(0);
            match ty {
                ValType::I32 => Value::I32(match pick {
                    0 => 0,
                    1 => -1,
                    2 => i32::MIN,
                    3 => i32::MAX,
                    _ => u.arbitrary().unwrap_or(0),
                }),
                ValType::I64 => Value::I64(match pick {
                    0 => 0,
                    1 => -1,
                    2 => i64::MIN,
                    3 => i64::MAX,
                    _ => u.arbitrary().unwrap_or(0),
                }),
                ValType::F32 => Value::F32(match pick {
                    0 => 0.0,
                    1 => f32::NAN,
                    2 => f32::INFINITY,
                    3 => f32::NEG_INFINITY,
                    _ => f32::from_bits(u.arbitrary().unwrap_or(0)),
                }),
                ValType::F64 => Value::F64(match pick {
                    0 => 0.0,
                    1 => f64::NAN,
                    2 => f64::INFINITY,
                    3 => f64::NEG_INFINITY,
                    _ => f64::from_bits(u.arbitrary().unwrap_or(0)),
                }),
            }
        })
        .collect()
}

/// Whether two outcomes agree. Float results compare by bit pattern, except
/// that any NaN matches any NaN: the spec leaves NaN payloads nondeterministic,
/// and `canonicalize_nans` only canonicalizes NaNs that reach memory or an
/// integer, not those returned directly.
#[cfg(feature = "differential")]
fn outcomes_agree(a: &ExecOutcome, b: &ExecOutcome) -> bool {
    match (a, b) {
        (ExecOutcome::Returned(Some(x)), ExecOutcome::Returned(Some(y))) => values_agree(x, y),
        _ => a == b,
    }
}

#[cfg(feature = "differential")]
fn values_agree(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::F32(x), Value::F32(y)) => (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits(),
        (Value::F64(x), Value::F64(y)) => (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits(),
        _ => a == b,
    }
}

#[cfg(feature = "differential")]
fn states_agree(a: &ExecState, b: &ExecState) -> bool {
    a.memory == b.memory
        && a.globals.len() == b.globals.len()
        && a.globals
            .iter()
            .zip(&b.globals)
            .all(|((na, va), (nb, vb))| na == nb && values_agree(va, vb))
}

/// The module running in SpaceWasm.
#[cfg(feature = "differential")]
struct SpaceExec {
    engine: Engine,
    code_builder: CodeBuilder,
    module_ref: ModuleRef,
    // Owns the guest memory, so it must outlive `engine`.
    _allocator: spacewasm::Rc<dyn WasmMemoryAllocator>,
}

#[cfg(feature = "differential")]
impl SpaceExec {
    /// Load the module and run its start function. `Err` means SpaceWasm refused
    /// the module at load time, carrying the rejection reason.
    fn instantiate(wasm: &[u8]) -> Result<(Self, ExecOutcome), ValidationError> {
        let mut engine = Engine::new(EXEC_STACK_WORDS, 16, Vec::zero())
            .expect("fuzz engine creation must not fail");
        let mut code_builder =
            CodeBuilder::new(CompilerOptions::new(true, None, MAX_CODE_PAGES)).unwrap();
        let mut stream = ByteStream::new(wasm);
        let allocator = spacewasm::Rc::new(FuzzAllocator::new())
            .unwrap()
            .into_wasm_memory_allocator();

        let mut control_frames = [ControlFrame::EMPTY; MAX_CONTROL_FRAMES];
        let mut operands = [OperandType::EMPTY; MAX_STACK_DEPTH];
        let mut decoder_stack = DecoderStack::new(&mut control_frames, &mut operands);
        let module = Module::new(
            "",
            &mut stream,
            &mut engine.store,
            &mut code_builder,
            &mut decoder_stack,
            allocator.clone(),
        )
        .map_err(|e: ParseError| e.err.err)?;
        let module_ref = engine
            .push_module(module)
            .expect("fuzz store has room for one module");

        let mut this = Self {
            engine,
            code_builder,
            module_ref,
            _allocator: allocator,
        };
        let start = match this.engine.module_start(module_ref) {
            None => ExecOutcome::Returned(None),
            Some(start) => this.run(start, &[], None),
        };
        Ok((this, start))
    }

    fn module(&self) -> &Module {
        &self.engine.store.modules()[self.module_ref.0 as usize]
    }

    /// The exported functions, with their parameter and result types.
    fn exported_funcs(&self) -> std::vec::Vec<(std::string::String, WasmRef, ExecSig)> {
        let module = self.module();
        module
            .exports
            .iter()
            .filter_map(|export| {
                let ExportDesc::Func(idx) = export.desc else {
                    return None;
                };
                // Generated modules have no imports, so every export is local.
                let Some(Ref::Module(index)) = module.get_func_ref(idx) else {
                    return None;
                };
                let func = &module.functions[index as usize];
                let ty = &module.types[func.ty.0 as usize];
                Some((
                    std::string::String::from(&*export.name),
                    WasmRef {
                        module: self.module_ref,
                        index,
                    },
                    ExecSig {
                        params: ty.params.iter().copied().collect(),
                        ret: ty.returns.first().copied(),
                    },
                ))
            })
            .collect()
    }

    fn run(&mut self, f: WasmRef, params: &[Value], ret: Option<ValType>) -> ExecOutcome {
        self.engine.reset();
        match self.engine.invoke(f, params) {
            Ok(()) => {}
            Err(InvokeError::StackOverflow) => return ExecOutcome::Exhausted,
            Err(e) => panic!("invoke of a generated export failed: {e:?}"),
        }
        match Interpreter.run(
            self.code_builder.pages(),
            &mut self.engine,
            EXEC_FUEL as usize,
        ) {
            InterpreterResult::Finished => ExecOutcome::Returned(ret.map(|ty| {
                self.engine
                    .result
                    .expect("a function with a result returned one")
                    .to_value(ty)
            })),
            InterpreterResult::OutOfFuel | InterpreterResult::Trap(TrapReason::StackOverflow) => {
                ExecOutcome::Exhausted
            }
            InterpreterResult::Trap(reason) => match Trap::from_spacewasm(reason) {
                Some(trap) => ExecOutcome::Trap(trap),
                None => panic!("SpaceWasm trap with no spec counterpart: {reason:?}"),
            },
            InterpreterResult::Pause => panic!("a module without imports paused"),
        }
    }

    fn state(&self) -> ExecState {
        let module = self.module();
        let mut globals = std::vec::Vec::new();
        let mut memory = None;
        for export in module.exports.iter() {
            match export.desc {
                ExportDesc::Global(idx) => globals.push((
                    std::string::String::from(&*export.name),
                    module.globals[idx.0 as usize].value(),
                )),
                ExportDesc::Mem(_) => {
                    memory = Some(
                        self.engine
                            .store
                            .get_memory(self.module_ref)
                            .get_slice()
                            .to_vec(),
                    );
                }
                _ => {}
            }
        }
        globals.sort_by(|a, b| a.0.cmp(&b.0));
        ExecState { globals, memory }
    }
}

/// Why wasmi could not produce a running instance.
#[cfg(feature = "differential")]
#[derive(Debug)]
enum WasmiInitError {
    /// The module failed to compile (validation disagreement, not an execution
    /// bug; `validate_differential` covers it).
    Compile,
    /// Instantiation failed before the start function: an element segment does
    /// not fit its table. SpaceWasm checks this at load.
    Instantiate,
    /// A data segment did not fit its memory, or the start function trapped or
    /// ran out of fuel.
    Start(ExecOutcome),
}

/// The module running in wasmi.
#[cfg(feature = "differential")]
struct WasmiExec {
    store: wasmi::Store<wasmi::StoreLimits>,
    instance: wasmi::Instance,
}

#[cfg(feature = "differential")]
impl WasmiExec {
    /// Instantiate and run the start function.
    fn instantiate(wasm: &[u8]) -> Result<(Self, ExecOutcome), WasmiInitError> {
        thread_local! {
            static ENGINE: wasmi::Engine = {
                let mut config = wasmi_config();
                config.consume_fuel(true);
                wasmi::Engine::new(&config)
            };
        }
        let engine = ENGINE.with(|e| e.clone());
        let module = wasmi::Module::new(&engine, wasm).map_err(|_| WasmiInitError::Compile)?;
        // Match SpaceWasm's guest-memory pool so `memory.grow` fails at the same
        // size in both engines.
        let limits = wasmi::StoreLimitsBuilder::new()
            .memory_size(GUEST_POOL_BYTES)
            .build();
        let mut store = wasmi::Store::new(&engine, limits);
        store.limiter(|limits| limits);
        store.set_fuel(EXEC_FUEL).unwrap();
        let linker = wasmi::Linker::<wasmi::StoreLimits>::new(&engine);
        match linker.instantiate_and_start(&mut store, &module) {
            Ok(instance) => Ok((Self { store, instance }, ExecOutcome::Returned(None))),
            // Out-of-bounds data segments surface as a memory trap code, like a
            // start-function trap; out-of-bounds element segments have none.
            Err(e) if e.as_trap_code().is_none() => Err(WasmiInitError::Instantiate),
            Err(e) => Err(WasmiInitError::Start(wasmi_error_outcome(&e))),
        }
    }

    fn run(&mut self, name: &str, params: &[Value], ret: Option<ValType>) -> ExecOutcome {
        let func = self
            .instance
            .get_func(&self.store, name)
            .expect("export present in both engines");
        self.store.set_fuel(EXEC_FUEL).unwrap();
        let inputs: std::vec::Vec<wasmi::Val> = params.iter().map(to_wasmi).collect();
        let mut outputs: std::vec::Vec<wasmi::Val> = ret
            .iter()
            .map(|ty| wasmi::Val::default_for_ty(wasmi_ty(*ty)))
            .collect();
        match func.call(&mut self.store, &inputs, &mut outputs) {
            Ok(()) => ExecOutcome::Returned(outputs.first().map(from_wasmi)),
            Err(e) => wasmi_error_outcome(&e),
        }
    }

    fn state(&self) -> ExecState {
        let mut globals = std::vec::Vec::new();
        let mut memory = None;
        for export in self.instance.exports(&self.store) {
            let name = std::string::String::from(export.name());
            match export.into_extern() {
                wasmi::Extern::Global(g) => globals.push((name, from_wasmi(&g.get(&self.store)))),
                wasmi::Extern::Memory(m) => memory = Some(m.data(&self.store).to_vec()),
                _ => {}
            }
        }
        globals.sort_by(|a, b| a.0.cmp(&b.0));
        ExecState { globals, memory }
    }
}

#[cfg(feature = "differential")]
fn wasmi_error_outcome(e: &wasmi::Error) -> ExecOutcome {
    use wasmi::TrapCode;
    match e.as_trap_code() {
        Some(TrapCode::OutOfFuel | TrapCode::StackOverflow) => ExecOutcome::Exhausted,
        Some(code) => match Trap::from_wasmi(code) {
            Some(trap) => ExecOutcome::Trap(trap),
            None => panic!("wasmi trap with no SpaceWasm counterpart: {code:?}"),
        },
        None => panic!("wasmi failed without a trap: {e}"),
    }
}

#[cfg(feature = "differential")]
fn wasmi_ty(ty: ValType) -> wasmi::ValType {
    match ty {
        ValType::I32 => wasmi::ValType::I32,
        ValType::I64 => wasmi::ValType::I64,
        ValType::F32 => wasmi::ValType::F32,
        ValType::F64 => wasmi::ValType::F64,
    }
}

#[cfg(feature = "differential")]
fn to_wasmi(v: &Value) -> wasmi::Val {
    match *v {
        Value::I32(x) => wasmi::Val::I32(x),
        Value::I64(x) => wasmi::Val::I64(x),
        Value::F32(x) => wasmi::Val::F32(x.into()),
        Value::F64(x) => wasmi::Val::F64(x.into()),
    }
}

#[cfg(feature = "differential")]
fn from_wasmi(v: &wasmi::Val) -> Value {
    match v {
        wasmi::Val::I32(x) => Value::I32(*x),
        wasmi::Val::I64(x) => Value::I64(*x),
        wasmi::Val::F32(x) => Value::F32(f32::from_bits(x.to_bits())),
        wasmi::Val::F64(x) => Value::F64(f64::from_bits(x.to_bits())),
        other => panic!("value of a type SpaceWasm does not support: {other:?}"),
    }
}

/// Oracle: differentially test *execution* against wasmi.
///
/// Instantiates the module in both engines, runs the start function, then calls
/// every exported function with arguments derived from `args`, comparing after
/// each step:
///
/// * the outcome -- the returned value, or which kind of trap was raised;
/// * the module state -- every exported global and the exported memory.
///
/// Calls run in export order against the same instance, so state carries over
/// from one call to the next in both engines. Once any step diverges, or either
/// engine exhausts its fuel or stack (whose limits differ between the engines),
/// the remaining calls are skipped: their starting state is no longer the same.
///
/// The module is skipped entirely when SpaceWasm rejects it at load for a reason
/// [`is_benign_rejection`] allows, which includes the instantiation-time
/// element/data bounds checks the spec defers past validation.
///
/// Best driven with the [`crate::generators::ExecModule`] generator, which allows
/// traps, so `call_indirect` and other trapping instructions are generated.
#[cfg(feature = "differential")]
pub fn exec_differential(wasm: &[u8], args: &[u8]) {
    crate::init_fuzzing();
    log_wasm(wasm);

    let space = SpaceExec::instantiate(wasm);
    let wasmi = WasmiExec::instantiate(wasm);
    let (mut space, mut wasmi) = match (space, wasmi) {
        (Ok((space, space_start)), Ok((wasmi, wasmi_start))) => {
            if !outcomes_agree(&space_start, &wasmi_start) {
                panic!(
                    "start function divergence: SpaceWasm {space_start:?}, wasmi {wasmi_start:?}"
                );
            }
            if space_start == ExecOutcome::Exhausted {
                return;
            }
            (space, wasmi)
        }
        // SpaceWasm traps in the start function report as `Ok` with a trap
        // outcome, wasmi's as an instantiation `Err`.
        (Ok((_, space_start)), Err(WasmiInitError::Start(wasmi_start))) => {
            if !outcomes_agree(&space_start, &wasmi_start)
                && space_start != ExecOutcome::Exhausted
                && wasmi_start != ExecOutcome::Exhausted
            {
                panic!(
                    "start function divergence: SpaceWasm {space_start:?}, wasmi {wasmi_start:?}"
                );
            }
            return;
        }
        // SpaceWasm refused the module at load.
        (Err(err), wasmi) => {
            if is_benign_rejection(&err) {
                return;
            }
            panic!(
                "SpaceWasm rejected ({err:?}) a generated module; wasmi: {:?}",
                wasmi.err()
            );
        }
        // wasmi refused to compile a module SpaceWasm accepted. Validation
        // disagreement is `validate_differential`'s job; not an execution bug.
        (Ok(_), Err(WasmiInitError::Compile)) => return,
        // An element segment wasmi found out of bounds, which SpaceWasm's load-time
        // check should also have rejected.
        (Ok(_), Err(WasmiInitError::Instantiate)) => {
            panic!("SpaceWasm instantiated a module whose element segments wasmi rejected as out of bounds");
        }
    };

    let state_space = space.state();
    let state_wasmi = wasmi.state();
    if !states_agree(&state_space, &state_wasmi) {
        panic!("state divergence after instantiation:\n  SpaceWasm {state_space:?}\n  wasmi     {state_wasmi:?}");
    }

    let mut u = arbitrary::Unstructured::new(args);
    let calls: std::vec::Vec<(ExecCall, WasmRef, Option<ValType>)> = space
        .exported_funcs()
        .into_iter()
        .map(|(name, f, ty)| {
            let params = exec_args(&mut u, &ty.params);
            (ExecCall { name, params }, f, ty.ret)
        })
        .collect();

    for (call, f, ret) in calls {
        let out_space = space.run(f, &call.params, ret);
        let out_wasmi = wasmi.run(&call.name, &call.params, ret);
        log::debug!(
            "call {:?}({:?}): SpaceWasm {out_space:?}, wasmi {out_wasmi:?}",
            call.name,
            call.params
        );
        if out_space == ExecOutcome::Exhausted || out_wasmi == ExecOutcome::Exhausted {
            return;
        }
        if !outcomes_agree(&out_space, &out_wasmi) {
            panic!(
                "execution divergence calling {:?}({:?}): SpaceWasm {out_space:?}, wasmi {out_wasmi:?}",
                call.name, call.params
            );
        }
        let state_space = space.state();
        let state_wasmi = wasmi.state();
        if !states_agree(&state_space, &state_wasmi) {
            panic!(
                "state divergence after calling {:?}({:?}):\n  SpaceWasm {state_space:?}\n  wasmi     {state_wasmi:?}",
                call.name, call.params
            );
        }
    }
}

#[cfg(all(test, feature = "differential"))]
mod exec_differential_tests {
    use super::*;

    fn outcomes(wat: &str, args: &[Value]) -> (ExecOutcome, ExecOutcome) {
        let wasm = wat::parse_str(wat).unwrap();
        let (mut space, _) = SpaceExec::instantiate(&wasm).unwrap();
        let (mut wasmi, _) = WasmiExec::instantiate(&wasm).unwrap();
        let (name, f, sig) = space.exported_funcs().remove(0);
        (space.run(f, args, sig.ret), wasmi.run(&name, args, sig.ret))
    }

    /// Both engines agree on a plain computation and on module state.
    #[test]
    fn exec_agrees_on_returns_and_state() {
        let wat = r#"(module
            (memory (export "m") 1 1)
            (global (export "g") (mut i32) (i32.const 7))
            (func (export "f") (param i32) (result i32)
                (i32.store (i32.const 8) (local.get 0))
                (global.set 0 (i32.add (global.get 0) (local.get 0)))
                (global.get 0)))"#;
        let (s, w) = outcomes(wat, &[Value::I32(5)]);
        assert_eq!(s, ExecOutcome::Returned(Some(Value::I32(12))));
        assert_eq!(s, w);
        exec_differential(&wat::parse_str(wat).unwrap(), &[]);
    }

    /// The three `call_indirect` traps map to the same kind in both engines.
    #[test]
    fn exec_classifies_call_indirect_traps() {
        let wat = r#"(module
            (type $v (func))
            (type $i (func (result i32)))
            (table 3 funcref)
            (elem (i32.const 0) $void $int)
            (func $void)
            (func $int (result i32) (i32.const 1))
            (func (export "call") (param i32) (call_indirect (type $v) (local.get 0))))"#;
        let cases = [
            (0, ExecOutcome::Returned(None)),
            (1, ExecOutcome::Trap(Trap::IndirectCallTypeMismatch)),
            (2, ExecOutcome::Trap(Trap::UninitializedElement)),
            (3, ExecOutcome::Trap(Trap::TableOutOfBounds)),
        ];
        for (idx, expected) in cases {
            let (s, w) = outcomes(wat, &[Value::I32(idx)]);
            assert_eq!(s, expected, "SpaceWasm, index {idx}");
            assert_eq!(w, expected, "wasmi, index {idx}");
        }
    }

    /// Arithmetic and conversion traps map to the same kind in both engines.
    #[test]
    fn exec_classifies_arithmetic_traps() {
        let cases = [
            ("i32.div_s (i32.const 1) (i32.const 0)", Trap::DivideByZero),
            (
                "i32.div_s (i32.const -2147483648) (i32.const -1)",
                Trap::IntegerOverflow,
            ),
            ("i32.trunc_f32_s (f32.const 1e10)", Trap::IntegerOverflow),
            (
                "i32.trunc_f32_s (f32.const nan)",
                Trap::BadConversionToInteger,
            ),
            ("i32.load (i32.const 65536)", Trap::MemoryOutOfBounds),
        ];
        for (expr, trap) in cases {
            let wat = format!(r#"(module (memory 1 1) (func (export "f") (result i32) ({expr})))"#);
            let (s, w) = outcomes(&wat, &[]);
            assert_eq!(s, ExecOutcome::Trap(trap), "SpaceWasm: {expr}");
            assert_eq!(w, ExecOutcome::Trap(trap), "wasmi: {expr}");
        }
    }

    /// An infinite loop exhausts fuel in both engines and is not compared.
    #[test]
    fn exec_skips_exhausted_calls() {
        let wat = r#"(module (func (export "f") (loop (br 0))))"#;
        let (s, w) = outcomes(wat, &[]);
        assert_eq!(s, ExecOutcome::Exhausted);
        assert_eq!(w, ExecOutcome::Exhausted);
        exec_differential(&wat::parse_str(wat).unwrap(), &[]);
    }

    /// A trapping start function agrees, and the oracle stays quiet.
    #[test]
    fn exec_agrees_on_start_trap() {
        let wasm = wat::parse_str(r#"(module (func $s unreachable) (start $s))"#).unwrap();
        exec_differential(&wasm, &[]);
    }

    /// Returned NaNs match regardless of payload; other floats compare by bits.
    #[test]
    fn exec_nan_results_agree_by_class() {
        let nan_a = ExecOutcome::Returned(Some(Value::F32(f32::from_bits(0x7fc0_0000))));
        let nan_b = ExecOutcome::Returned(Some(Value::F32(f32::from_bits(0xffc0_0001))));
        assert!(outcomes_agree(&nan_a, &nan_b));
        let pos = ExecOutcome::Returned(Some(Value::F64(0.0)));
        let neg = ExecOutcome::Returned(Some(Value::F64(-0.0)));
        assert!(!outcomes_agree(&pos, &neg));
    }
}
