mod util;
use util::run_wast_test_file;

#[test]
fn address() {
    run_wast_test_file("core/address");
}

#[test]
fn call() {
    run_wast_test_file("core/call");
}

#[test]
fn exports() {
    run_wast_test_file("core/exports");
}

#[test]
fn float_literals() {
    run_wast_test_file("core/float_literals");
}

#[test]
fn if_() {
    run_wast_test_file("core/if");
}

#[test]
fn local_get() {
    run_wast_test_file("core/local_get");
}

#[test]
fn names() {
    run_wast_test_file("core/names");
}

#[test]
fn table() {
    run_wast_test_file("core/table");
}

#[test]
fn utf8_import_module() {
    run_wast_test_file("core/utf8-import-module");
}

#[test]
fn align() {
    run_wast_test_file("core/align");
}

#[test]
fn call_indirect() {
    run_wast_test_file("core/call_indirect");
}

#[test]
fn f32() {
    run_wast_test_file("core/f32");
}

#[test]
fn float_memory() {
    run_wast_test_file("core/float_memory");
}

#[test]
fn imports() {
    run_wast_test_file("core/imports");
}

#[test]
fn local_set() {
    run_wast_test_file("core/local_set");
}

#[test]
fn nop() {
    run_wast_test_file("core/nop");
}

#[test]
fn token() {
    run_wast_test_file("core/token");
}

#[test]
fn utf8_invalid_encoding() {
    run_wast_test_file("core/utf8-invalid-encoding");
}

#[test]
fn binary() {
    run_wast_test_file("core/binary");
}

#[test]
fn comments() {
    run_wast_test_file("core/comments");
}

#[test]
fn f32_bitwise() {
    run_wast_test_file("core/f32_bitwise");
}

#[test]
fn float_misc() {
    run_wast_test_file("core/float_misc");
}

#[test]
fn inline_module() {
    run_wast_test_file("core/inline-module");
}

#[test]
fn local_tee() {
    run_wast_test_file("core/local_tee");
}

#[test]
fn return_() {
    run_wast_test_file("core/return");
}

#[test]
fn traps() {
    run_wast_test_file("core/traps");
}

#[test]
fn binary_leb128() {
    run_wast_test_file("core/binary-leb128");
}

#[test]
fn const_() {
    run_wast_test_file("core/const");
}

#[test]
fn f32_cmp() {
    run_wast_test_file("core/f32_cmp");
}

#[test]
fn forward() {
    run_wast_test_file("core/forward");
}

#[test]
fn int_exprs() {
    run_wast_test_file("core/int_exprs");
}

#[test]
fn loop_() {
    run_wast_test_file("core/loop");
}

#[test]
fn select() {
    run_wast_test_file("core/select");
}

#[test]
fn type_() {
    run_wast_test_file("core/type");
}

#[test]
fn block() {
    run_wast_test_file("core/block");
}

#[test]
fn conversions() {
    run_wast_test_file("core/conversions");
}

#[test]
fn f64() {
    run_wast_test_file("core/f64");
}

#[test]
fn func() {
    run_wast_test_file("core/func");
}

#[test]
fn int_literals() {
    run_wast_test_file("core/int_literals");
}

#[test]
fn memory() {
    run_wast_test_file("core/memory");
}

#[test]
fn skip_stack_guard_page() {
    run_wast_test_file("core/skip-stack-guard-page");
}

#[test]
fn unreachable() {
    run_wast_test_file("core/unreachable");
}

#[test]
fn br() {
    run_wast_test_file("core/br");
}

#[test]
fn custom() {
    run_wast_test_file("core/custom");
}

#[test]
fn f64_bitwise() {
    run_wast_test_file("core/f64_bitwise");
}

#[test]
fn func_ptrs() {
    run_wast_test_file("core/func_ptrs");
}

#[test]
fn labels() {
    run_wast_test_file("core/labels");
}

#[test]
fn memory_grow() {
    run_wast_test_file("core/memory_grow");
}

#[test]
fn stack() {
    run_wast_test_file("core/stack");
}

#[test]
fn unreached_invalid() {
    run_wast_test_file("core/unreached-invalid");
}

#[test]
fn br_if() {
    run_wast_test_file("core/br_if");
}

#[test]
fn data() {
    run_wast_test_file("core/data");
}

#[test]
fn f64_cmp() {
    run_wast_test_file("core/f64_cmp");
}

#[test]
fn global() {
    run_wast_test_file("core/global");
}

#[test]
fn left_to_right() {
    run_wast_test_file("core/left-to-right");
}

#[test]
fn memory_redundancy() {
    run_wast_test_file("core/memory_redundancy");
}

#[test]
fn start() {
    run_wast_test_file("core/start");
}

#[test]
fn unwind() {
    run_wast_test_file("core/unwind");
}

#[test]
fn br_table() {
    run_wast_test_file("core/br_table");
}

#[test]
fn elem() {
    run_wast_test_file("core/elem");
}

#[test]
fn fac() {
    run_wast_test_file("core/fac");
}

#[test]
fn i32() {
    run_wast_test_file("core/i32");
}

#[test]
fn linking() {
    run_wast_test_file("core/linking");
}

#[test]
fn memory_size() {
    run_wast_test_file("core/memory_size");
}

#[test]
fn store() {
    run_wast_test_file("core/store");
}

#[test]
fn utf8_custom_section_id() {
    run_wast_test_file("core/utf8-custom-section-id");
}

#[test]
fn break_drop() {
    run_wast_test_file("core/break-drop");
}

#[test]
fn endianness() {
    run_wast_test_file("core/endianness");
}

#[test]
fn float_exprs() {
    run_wast_test_file("core/float_exprs");
}

#[test]
fn i64() {
    run_wast_test_file("core/i64");
}

#[test]
fn load() {
    run_wast_test_file("core/load");
}

#[test]
fn memory_trap() {
    run_wast_test_file("core/memory_trap");
}

#[test]
fn switch() {
    run_wast_test_file("core/switch");
}

#[test]
fn utf8_import_field() {
    run_wast_test_file("core/utf8-import-field");
}

#[test]
fn test() {
    run_wast_test_file("core/test");
}
