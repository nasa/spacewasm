mod spectest;
use spectest::*;

#[test]
fn address() {
    run_wast_test_file("address");
}

#[test]
fn align() {
    run_wast_test_file("align");
}

#[test]
fn annotations() {
    run_wast_test_file("annotations");
}

#[test]
fn binary_leb128() {
    run_wast_test_file("binary-leb128");
}

#[test]
fn binary() {
    run_wast_test_file("binary");
}

#[test]
fn block() {
    run_wast_test_file("block");
}

#[test]
fn br_if() {
    run_wast_test_file("br_if");
}

#[test]
fn br_on_non_null() {
    run_wast_test_file("br_on_non_null");
}

#[test]
fn br_on_null() {
    run_wast_test_file("br_on_null");
}

#[test]
fn br_table() {
    run_wast_test_file("br_table");
}

#[test]
fn br() {
    run_wast_test_file("br");
}

#[test]
fn call_indirect() {
    run_wast_test_file("call_indirect");
}

#[test]
fn call_ref() {
    run_wast_test_file("call_ref");
}

#[test]
fn call() {
    run_wast_test_file("call");
}

#[test]
fn comments() {
    run_wast_test_file("comments");
}

#[test]
fn const_() {
    run_wast_test_file("const");
}

#[test]
fn conversions() {
    run_wast_test_file("conversions");
}

#[test]
fn custom() {
    run_wast_test_file("custom");
}

#[test]
fn data() {
    run_wast_test_file("data");
}

#[test]
fn elem() {
    run_wast_test_file("elem");
}

#[test]
fn endianness() {
    run_wast_test_file("endianness");
}

#[test]
fn exports() {
    run_wast_test_file("exports");
}

#[test]
fn f32_bitwise() {
    run_wast_test_file("f32_bitwise");
}

#[test]
fn f32_cmp() {
    run_wast_test_file("f32_cmp");
}

#[test]
fn f32() {
    run_wast_test_file("f32");
}

#[test]
fn f64_bitwise() {
    run_wast_test_file("f64_bitwise");
}

#[test]
fn f64_cmp() {
    run_wast_test_file("f64_cmp");
}

#[test]
fn f64() {
    run_wast_test_file("f64");
}

#[test]
fn fac() {
    run_wast_test_file("fac");
}

#[test]
fn float_exprs() {
    run_wast_test_file("float_exprs");
}

#[test]
fn float_literals() {
    run_wast_test_file("float_literals");
}

#[test]
fn float_memory() {
    run_wast_test_file("float_memory");
}

#[test]
fn float_misc() {
    run_wast_test_file("float_misc");
}

#[test]
fn forward() {
    run_wast_test_file("forward");
}

#[test]
fn func_ptrs() {
    run_wast_test_file("func_ptrs");
}

#[test]
fn func() {
    run_wast_test_file("func");
}

#[test]
fn global() {
    run_wast_test_file("global");
}

#[test]
fn i32() {
    run_wast_test_file("i32");
}

#[test]
fn i64() {
    run_wast_test_file("i64");
}

#[test]
fn id() {
    run_wast_test_file("id");
}

#[test]
fn if_() {
    run_wast_test_file("if");
}

#[test]
fn imports() {
    run_wast_test_file("imports");
}

#[test]
fn inline_module() {
    run_wast_test_file("inline-module");
}

#[test]
fn instance() {
    run_wast_test_file("instance");
}

#[test]
fn int_exprs() {
    run_wast_test_file("int_exprs");
}

#[test]
fn int_literals() {
    run_wast_test_file("int_literals");
}

#[test]
fn labels() {
    run_wast_test_file("labels");
}

#[test]
fn left_to_right() {
    run_wast_test_file("left-to-right");
}

#[test]
fn linking() {
    run_wast_test_file("linking");
}

#[test]
fn load() {
    run_wast_test_file("load");
}

#[test]
fn local_get() {
    run_wast_test_file("local_get");
}

#[test]
fn local_init() {
    run_wast_test_file("local_init");
}

#[test]
fn local_set() {
    run_wast_test_file("local_set");
}

#[test]
fn local_tee() {
    run_wast_test_file("local_tee");
}

#[test]
fn loop_() {
    run_wast_test_file("loop");
}

#[test]
fn memory_grow() {
    run_wast_test_file("memory_grow");
}

#[test]
fn memory_redundancy() {
    run_wast_test_file("memory_redundancy");
}

#[test]
fn memory_size() {
    run_wast_test_file("memory_size");
}

#[test]
fn memory_trap() {
    run_wast_test_file("memory_trap");
}

#[test]
fn memory() {
    run_wast_test_file("memory");
}

#[test]
fn names() {
    run_wast_test_file("names");
}

#[test]
fn nop() {
    run_wast_test_file("nop");
}

#[test]
fn obsolete_keywords() {
    run_wast_test_file("obsolete-keywords");
}

#[test]
fn ref_as_non_null() {
    run_wast_test_file("ref_as_non_null");
}

#[test]
fn ref_func() {
    run_wast_test_file("ref_func");
}

#[test]
fn ref_is_null() {
    run_wast_test_file("ref_is_null");
}

#[test]
fn ref_null() {
    run_wast_test_file("ref_null");
}

#[test]
fn ref_() {
    run_wast_test_file("ref");
}

#[test]
fn return_call_indirect() {
    run_wast_test_file("return_call_indirect");
}

#[test]
fn return_call_ref() {
    run_wast_test_file("return_call_ref");
}

#[test]
fn return_call() {
    run_wast_test_file("return_call");
}

#[test]
fn return_() {
    run_wast_test_file("return");
}

#[test]
fn select() {
    run_wast_test_file("select");
}

#[test]
fn skip_stack_guard_page() {
    run_wast_test_file("skip-stack-guard-page");
}

#[test]
fn stack() {
    run_wast_test_file("stack");
}

#[test]
fn start() {
    run_wast_test_file("start");
}

#[test]
fn store() {
    run_wast_test_file("store");
}

#[test]
fn switch() {
    run_wast_test_file("switch");
}

#[test]
fn table_get() {
    run_wast_test_file("table_get");
}

#[test]
fn table_grow() {
    run_wast_test_file("table_grow");
}

#[test]
fn table_set() {
    run_wast_test_file("table_set");
}

#[test]
fn table_size() {
    run_wast_test_file("table_size");
}

#[test]
fn table() {
    run_wast_test_file("table");
}

#[test]
fn token() {
    run_wast_test_file("token");
}

#[test]
fn traps() {
    run_wast_test_file("traps");
}

#[test]
fn type_canon() {
    run_wast_test_file("type-canon");
}

#[test]
fn type_equivalence() {
    run_wast_test_file("type-equivalence");
}

#[test]
fn type_rec() {
    run_wast_test_file("type-rec");
}

#[test]
fn type_() {
    run_wast_test_file("type");
}

#[test]
fn unreachable() {
    run_wast_test_file("unreachable");
}

#[test]
fn unreached_invalid() {
    run_wast_test_file("unreached-invalid");
}

#[test]
fn unreached_valid() {
    run_wast_test_file("unreached-valid");
}

#[test]
fn unwind() {
    run_wast_test_file("unwind");
}

#[test]
fn utf8_custom_section_id() {
    run_wast_test_file("utf8-custom-section-id");
}

#[test]
fn utf8_import_field() {
    run_wast_test_file("utf8-import-field");
}

#[test]
fn utf8_import_module() {
    run_wast_test_file("utf8-import-module");
}

#[test]
fn utf8_invalid_encoding() {
    run_wast_test_file("utf8-invalid-encoding");
}
