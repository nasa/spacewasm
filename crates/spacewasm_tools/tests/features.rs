//! `PROPOSALS` held to the decoder: every proposal it calls supported must decode, and
//! every one it calls unsupported must be refused. A proposal added to `spacewasm` and
//! not marked here, or the reverse, fails this rather than shipping the wrong flags.

use spacewasm::{CompilerOptions, PageAllocator};
use spacewasm_tools::{DecoderLimits, Invalid, PROPOSALS, host_modules_for, validate};

spacewasm::global_allocator!(
    PageAllocator<System, 64>,
    PageAllocator::new(System, 64 * 1024)
);

struct System;

// SAFETY: forwards to the Rust global allocator.
unsafe impl spacewasm::Allocator for System {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> Result<*mut u8, spacewasm::AllocError> {
        // SAFETY: the caller guarantees a non-zero size.
        let ptr = unsafe { std::alloc::alloc(layout) };
        match ptr.is_null() {
            true => Err(spacewasm::AllocError::AllocationFailed),
            false => Ok(ptr),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        // SAFETY: `ptr` came from `alloc` with `layout`.
        unsafe { std::alloc::dealloc(ptr, layout) }
    }
}

/// One module per proposal, using an instruction (or, for the two that add none, a
/// construct) only that proposal allows.
fn sample(proposal: &str) -> &'static str {
    match proposal {
        "mutable-globals" => {
            r#"(module (import "env" "g" (global (mut i32)))
                 (func (export "main") global.get 0 drop))"#
        }
        "custom-page-sizes" => r#"(module (memory 1 (pagesize 1)) (func (export "main")))"#,
        "sign-extension-ops" => {
            r#"(module (func (export "main") (param i32) (result i32)
                 local.get 0 i32.extend8_s))"#
        }
        "nontrapping-float-to-int-conversions" => {
            r#"(module (func (export "main") (param f32) (result i32)
                 local.get 0 i32.trunc_sat_f32_s))"#
        }
        "bulk-memory-operations" => {
            r#"(module (memory 1) (func (export "main")
                 i32.const 0 i32.const 0 i32.const 1 memory.fill))"#
        }
        "reference-types" => r#"(module (func (export "main") ref.null func drop))"#,
        "multi-value" => {
            r#"(module (func (export "main") (result i32 i32) i32.const 1 i32.const 2))"#
        }
        "extended-const" => {
            r#"(module (global i32 (i32.add (i32.const 1) (i32.const 2)))
                 (func (export "main")))"#
        }
        "simd" => r#"(module (func (export "main") v128.const i64x2 0 0 drop))"#,
        "threads" => {
            r#"(module (memory 1 1 shared) (func (export "main")
                 i32.const 0 i32.atomic.load drop))"#
        }
        "tail-call" => r#"(module (func $f) (func (export "main") return_call $f))"#,
        other => panic!("no sample for `{other}`: add one to keep the table checked"),
    }
}

#[test]
fn the_table_matches_the_decoder() {
    let mut wrong = Vec::new();
    for proposal in PROPOSALS {
        let module = wat::parse_str(sample(proposal.name)).expect("the sample assembles");
        let hosts = host_modules_for(&module).expect("stand-in hosts");
        let options = CompilerOptions::new(true, None, 16);
        let loads = validate(&module, options, hosts, DecoderLimits::default());

        match (proposal.supported, &loads) {
            (true, Ok(_)) | (false, Err(_)) => {}
            (true, Err(err)) => {
                wrong.push(format!("{}: marked supported, but {err}", proposal.name))
            }
            (false, Ok(_)) => wrong.push(format!(
                "{}: marked unsupported, but it decodes. Mark it supported so `link` and \
                 `features` use it",
                proposal.name
            )),
        }
        // An unsupported instruction should be named, not reported as a bare opcode.
        if let Err(Invalid::Unsupported {
            proposal: named, ..
        }) = &loads
        {
            assert_eq!(named.name, proposal.name, "named the wrong proposal");
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}
