mod util;
use util::run_wast_test_file;

#[test]
fn address() {
    run_wast_test_file("address");
}

#[test]
fn call() {
    run_wast_test_file("call");
}

#[test]
fn exports() {
    run_wast_test_file("exports");
}

#[test]
fn float_literals() {
    run_wast_test_file("float_literals");
}

#[test]
fn if_() {
    run_wast_test_file("if");
}

#[test]
fn local_get() {
    run_wast_test_file("local_get");
}

#[test]
fn names() {
    run_wast_test_file("names");
}

#[test]
fn table() {
    run_wast_test_file("table");
}

#[test]
fn utf8_import_module() {
    run_wast_test_file("utf8-import-module");
}

#[test]
fn align() {
    run_wast_test_file("align");
}

#[test]
fn call_indirect() {
    run_wast_test_file("call_indirect");
}

#[test]
fn f32() {
    run_wast_test_file("f32");
}

#[test]
fn float_memory() {
    run_wast_test_file("float_memory");
}

#[test]
fn imports() {
    run_wast_test_file("imports");
}

#[test]
fn local_set() {
    run_wast_test_file("local_set");
}

#[test]
fn nop() {
    run_wast_test_file("nop");
}

#[test]
fn token() {
    run_wast_test_file("token");
}

#[test]
fn utf8_invalid_encoding() {
    run_wast_test_file("utf8-invalid-encoding");
}

#[test]
fn binary() {
    run_wast_test_file("binary");
}

#[test]
fn comments() {
    run_wast_test_file("comments");
}

#[test]
fn f32_bitwise() {
    run_wast_test_file("f32_bitwise");
}

#[test]
fn float_misc() {
    run_wast_test_file("float_misc");
}

#[test]
fn inline_module() {
    run_wast_test_file("inline-module");
}

#[test]
fn local_tee() {
    run_wast_test_file("local_tee");
}

#[test]
fn return_() {
    run_wast_test_file("return");
}

#[test]
fn traps() {
    run_wast_test_file("traps");
}

#[test]
fn binary_leb128() {
    run_wast_test_file("binary-leb128");
}

#[test]
fn const_() {
    run_wast_test_file("const");
}

#[test]
fn f32_cmp() {
    run_wast_test_file("f32_cmp");
}

#[test]
fn forward() {
    run_wast_test_file("forward");
}

#[test]
fn int_exprs() {
    run_wast_test_file("int_exprs");
}

#[test]
fn loop_() {
    run_wast_test_file("loop");
}

#[test]
fn select() {
    run_wast_test_file("select");
}

#[test]
fn type_() {
    run_wast_test_file("type");
}

#[test]
fn block() {
    run_wast_test_file("block");
}

#[test]
fn conversions() {
    run_wast_test_file("conversions");
}

#[test]
fn f64() {
    run_wast_test_file("f64");
}

#[test]
fn func() {
    run_wast_test_file("func");
}

#[test]
fn int_literals() {
    run_wast_test_file("int_literals");
}

#[test]
fn memory() {
    run_wast_test_file("memory");
}

#[test]
fn skip_stack_guard_page() {
    run_wast_test_file("skip-stack-guard-page");
}

#[test]
fn unreachable() {
    run_wast_test_file("unreachable");
}

#[test]
fn br() {
    run_wast_test_file("br");
}

#[test]
fn custom() {
    run_wast_test_file("custom");
}

#[test]
fn f64_bitwise() {
    run_wast_test_file("f64_bitwise");
}

#[test]
fn func_ptrs() {
    run_wast_test_file("func_ptrs");
}

#[test]
fn labels() {
    run_wast_test_file("labels");
}

#[test]
fn memory_grow() {
    run_wast_test_file("memory_grow");
}

#[test]
fn stack() {
    run_wast_test_file("stack");
}

#[test]
fn unreached_invalid() {
    run_wast_test_file("unreached-invalid");
}

#[test]
fn br_if() {
    run_wast_test_file("br_if");
}

#[test]
fn data() {
    run_wast_test_file("data");
}

#[test]
fn f64_cmp() {
    run_wast_test_file("f64_cmp");
}

#[test]
fn global() {
    run_wast_test_file("global");
}

#[test]
fn left_to_right() {
    run_wast_test_file("left-to-right");
}

#[test]
fn memory_redundancy() {
    run_wast_test_file("memory_redundancy");
}

#[test]
fn start() {
    run_wast_test_file("start");
}

#[test]
fn unwind() {
    run_wast_test_file("unwind");
}

#[test]
fn br_table() {
    run_wast_test_file("br_table");
}

#[test]
fn elem() {
    run_wast_test_file("elem");
}

#[test]
fn fac() {
    run_wast_test_file("fac");
}

#[test]
fn i32() {
    run_wast_test_file("i32");
}

#[test]
fn linking() {
    run_wast_test_file("linking");
}

#[test]
fn memory_size() {
    run_wast_test_file("memory_size");
}

#[test]
fn store() {
    run_wast_test_file("store");
}

#[test]
fn utf8_custom_section_id() {
    run_wast_test_file("utf8-custom-section-id");
}

#[test]
fn break_drop() {
    run_wast_test_file("break-drop");
}

#[test]
fn endianness() {
    run_wast_test_file("endianness");
}

#[test]
fn float_exprs() {
    run_wast_test_file("float_exprs");
}

#[test]
fn i64() {
    run_wast_test_file("i64");
}

#[test]
fn load() {
    run_wast_test_file("load");
}

#[test]
fn memory_trap() {
    run_wast_test_file("memory_trap");
}

#[test]
fn switch() {
    run_wast_test_file("switch");
}

#[test]
fn utf8_import_field() {
    run_wast_test_file("utf8-import-field");
}
