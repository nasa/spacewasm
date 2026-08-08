mod util;
use spacewasm::vec;
use util::{run_wast_test_file, spectest_host_module};

fn run(test_name: &str) {
    run_wast_test_file(test_name, || vec![spectest_host_module()]);
}

#[test]
fn address() {
    run("core/address");
}

#[test]
#[cfg_attr(miri, ignore = "stack recursion")]
fn call() {
    run("core/call");
}

#[test]
fn exports() {
    run("core/exports");
}

#[test]
fn float_literals() {
    run("core/float_literals");
}

#[test]
fn if_() {
    run("core/if");
}

#[test]
fn local_get() {
    run("core/local_get");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn names() {
    run("core/names");
}

#[test]
fn table() {
    run("core/table");
}

#[test]
fn utf8_import_module() {
    run("core/utf8-import-module");
}

#[test]
fn align() {
    run("core/align");
}

#[test]
#[cfg_attr(miri, ignore = "stack recursion")]
fn call_indirect() {
    run("core/call_indirect");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn f32() {
    run("core/f32");
}

#[test]
fn float_memory() {
    run("core/float_memory");
}

#[test]
fn imports() {
    run("core/imports");
}

#[test]
fn local_set() {
    run("core/local_set");
}

#[test]
fn nop() {
    run("core/nop");
}

#[test]
fn token() {
    run("core/token");
}

#[test]
fn utf8_invalid_encoding() {
    run("core/utf8-invalid-encoding");
}

#[test]
fn binary() {
    run("core/binary");
}

#[test]
fn comments() {
    run("core/comments");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn f32_bitwise() {
    run("core/f32_bitwise");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn float_misc() {
    run("core/float_misc");
}

#[test]
fn inline_module() {
    run("core/inline-module");
}

#[test]
fn local_tee() {
    run("core/local_tee");
}

#[test]
fn return_() {
    run("core/return");
}

#[test]
fn traps() {
    run("core/traps");
}

#[test]
fn binary_leb128() {
    run("core/binary-leb128");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn const_() {
    run("core/const");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn f32_cmp() {
    run("core/f32_cmp");
}

#[test]
fn forward() {
    run("core/forward");
}

#[test]
fn int_exprs() {
    run("core/int_exprs");
}

#[test]
#[cfg_attr(miri, ignore = "long runtime")]
fn loop_() {
    run("core/loop");
}

#[test]
fn select() {
    run("core/select");
}

#[test]
fn type_() {
    run("core/type");
}

#[test]
fn block() {
    run("core/block");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn conversions() {
    run("core/conversions");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn f64() {
    run("core/f64");
}

#[test]
fn func() {
    run("core/func");
}

#[test]
fn int_literals() {
    run("core/int_literals");
}

#[test]
fn memory() {
    run("core/memory");
}

#[test]
#[cfg_attr(miri, ignore = "stack recursion")]
fn skip_stack_guard_page() {
    run("core/skip-stack-guard-page");
}

#[test]
fn unreachable() {
    run("core/unreachable");
}

#[test]
fn br() {
    run("core/br");
}

#[test]
fn custom() {
    run("core/custom");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn f64_bitwise() {
    run("core/f64_bitwise");
}

#[test]
fn func_ptrs() {
    run("core/func_ptrs");
}

#[test]
fn labels() {
    run("core/labels");
}

#[test]
#[cfg_attr(miri, ignore = "malloc too slow")]
fn memory_grow() {
    run("core/memory_grow");
}

#[test]
fn stack() {
    run("core/stack");
}

#[test]
fn unreached_invalid() {
    run("core/unreached-invalid");
}

#[test]
fn br_if() {
    run("core/br_if");
}

#[test]
fn data() {
    run("core/data");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn f64_cmp() {
    run("core/f64_cmp");
}

#[test]
fn global() {
    run("core/global");
}

#[test]
fn left_to_right() {
    run("core/left-to-right");
}

#[test]
fn memory_redundancy() {
    run("core/memory_redundancy");
}

#[test]
fn start() {
    run("core/start");
}

#[test]
fn unwind() {
    run("core/unwind");
}

#[test]
#[cfg_attr(miri, ignore = "long runtime")]
fn br_table() {
    run("core/br_table");
}

#[test]
fn elem() {
    run("core/elem");
}

#[test]
#[cfg_attr(miri, ignore = "stack recursion")]
fn fac() {
    run("core/fac");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn i32() {
    run("core/i32");
}

#[test]
fn linking() {
    run("core/linking");
}

#[test]
fn memory_size() {
    run("core/memory_size");
}

#[test]
fn store() {
    run("core/store");
}

#[test]
fn utf8_custom_section_id() {
    run("core/utf8-custom-section-id");
}

#[test]
fn break_drop() {
    run("core/break-drop");
}

#[test]
fn endianness() {
    run("core/endianness");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn float_exprs() {
    run("core/float_exprs");
}

#[test]
#[cfg_attr(miri, ignore = "libm too slow")]
fn i64() {
    run("core/i64");
}

#[test]
fn load() {
    run("core/load");
}

#[test]
fn memory_trap() {
    run("core/memory_trap");
}

#[test]
fn switch() {
    run("core/switch");
}

#[test]
fn utf8_import_field() {
    run("core/utf8-import-field");
}

#[test]
fn test() {
    run("core/test");
}
