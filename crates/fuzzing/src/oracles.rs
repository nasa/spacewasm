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
            let inner = InnerVec {
                ptr: vec.as_mut_ptr(),
                capacity: vec.len() as u32,
                len: vec.len() as u32,
            };
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
            Err(AllocError::AllocationFailed)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr, layout) }
    }

    fn memory_statistics(&self) -> MemoryStatistics {
        MemoryStatistics {
            total_bytes: 0,
            pad_bytes: 0,
        }
    }
}

/// A model of the production `PageAllocator` for fuzzing, backing guest memory.
///
/// Like the `RustSystemAllocator` used by the integration tests, this is a thin
/// passthrough to the system allocator with no running-total bookkeeping. Its
/// only policy is the same per-allocation size cap ([`MAX_ALLOCATION_BYTES`])
/// applied by [`SystemAllocator`], so a single unreasonable request from a
/// corrupted module is rejected rather than aborting the fuzzer.
pub(crate) struct FuzzAllocator;

impl WasmMemoryAllocator for FuzzAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, AllocError> {
        if layout.size() > MAX_ALLOCATION_BYTES {
            return Err(AllocError::OutOfMemory);
        }
        unsafe { NonNull::new(std::alloc::alloc(layout)).ok_or(AllocError::AllocationFailed) }
    }

    fn reallocate(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        layout: Layout,
    ) -> Result<NonNull<u8>, AllocError> {
        if layout.size() > MAX_ALLOCATION_BYTES {
            return Err(AllocError::OutOfMemory);
        }
        unsafe {
            NonNull::new(std::alloc::realloc(ptr.as_ptr(), old_layout, layout.size()))
                .ok_or(AllocError::AllocationFailed)
        }
    }

    fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) }
    }
}

// Set up the global allocator for fuzzing
#[allow(missing_docs)]
mod fuzz_alloc {
    use super::SystemAllocator;
    spacewasm::global_allocator!(SystemAllocator, SystemAllocator);
}

const MAX_CODE_PAGES: u32 = 128;
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

    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: true,
        max_backpatch_iterations: 0,
        max_code_pages: MAX_CODE_PAGES,
    })
    .unwrap();
    let mut stream = ByteStream::new(wasm);

    let allocator = spacewasm::Rc::new(FuzzAllocator)
        .unwrap()
        .into_wasm_memory_allocator();

    // Attempt to decode and validate the module. `ParseError` wraps a
    // `SectionDecodeError`, which in turn carries the innermost `ValidationError`.
    Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "",
        &mut stream,
        &mut store,
        &mut code_builder,
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
/// out-of-bounds active data segments (`InvalidNegativeMemOffset`, or a
/// `MemoryError` from the eager memory write).
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
/// `BlockResultTypeMismatch`).
/// See <https://github.com/nasa/spacewasm/issues/171#issuecomment-5333770458>.
///
/// Variant granularity is sound for the first three groups because this classifier
/// is only consulted when wasmi *accepted* the module: the validation-error
/// subcases that share those variants (e.g. a non-`i32` element/data offset
/// expression, an unknown section id other than 12, or an export of a nonexistent
/// memory index) make wasmi reject too, so they never reach this check.
///
/// `BlockResultTypeMismatch` is coarser. SpaceWasm also emits it for block-end
/// stack-height mismatches and a result-typed `if` without an `else` -- errors
/// invalid under *both* spec versions, so wasmi rejects them too and they do not
/// reach this check today. But that safety rests on SpaceWasm and wasmi agreeing,
/// not on a spec relaxation, so allow-listing the whole variant gives up the
/// fuzzer's ability to flag a *hypothetical* SpaceWasm over-strictness bug that
/// rejected an otherwise-1.0-valid module with this same variant (e.g. a dead-code
/// stack-height check). Narrowing it to only the `br_table` subcase would require
/// a dedicated error variant in the production validator; this classifier can only
/// discriminate on the variant it is handed.
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
            | ValidationError::InvalidNegativeMemOffset
            // wasmi feature-gating leniency (bulk-memory encodings it accepts
            // even with the proposal disabled).
            | ValidationError::MalformedSectionId(12)
            | ValidationError::InvalidMemIndex
            // Spec-version difference: a `br_table` whose targets share arity but
            // differ in type is valid under wasm 2.0 (wasmi) and invalid under wasm
            // 1.0 (SpaceWasm). Coarse -- this variant also covers block-end checks
            // invalid under both versions; see the doc comment above.
            | ValidationError::BlockResultTypeMismatch
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
    let mut code_builder = CodeBuilder::new(CompilerOptions {
        allow_memory_grow: true,
        max_backpatch_iterations: 0,
        max_code_pages: MAX_CODE_PAGES,
    })
    .unwrap();
    let mut stream = ByteStream::new(wasm);

    let allocator = Rc::new(FuzzAllocator).unwrap().into_wasm_memory_allocator();

    let module = match Module::new::<MAX_CONTROL_FRAMES, MAX_STACK_DEPTH>(
        "",
        &mut stream,
        &mut state.store,
        &mut code_builder,
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
        state.invoke(wasm_ref, &params).unwrap();

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
/// Matched to SpaceWasm's accepted language: Wasm 1.0 MVP **plus custom page
/// sizes** (SpaceWasm's one supported post-MVP proposal). Feature parity matters
/// here -- a feature one validator enables and the other doesn't would make them
/// disagree for reasons that are not bugs. Fuel is irrelevant to validation.
#[cfg(feature = "differential")]
fn build_wasmi_validate_engine() -> wasmi::Engine {
    let mut config = wasmi::Config::default();
    config
        .wasm_mutable_global(true)
        .floats(true)
        // SpaceWasm supports custom page sizes; enable it so the two validators
        // agree on modules that use them.
        .wasm_custom_page_sizes(true)
        // Everything else post-MVP: off.
        .wasm_multi_value(false)
        .wasm_multi_memory(false)
        .wasm_sign_extension(false)
        .wasm_saturating_float_to_int(false)
        .wasm_bulk_memory(false)
        .wasm_reference_types(false)
        .wasm_tail_call(false)
        .wasm_extended_const(false)
        .wasm_memory64(false)
        .wasm_wide_arithmetic(false);
    wasmi::Engine::new(&config)
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
