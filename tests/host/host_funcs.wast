;; Host function calls and return values

(module
  (import "host" "return_i32_from_all_args"
    (func $return_i32_from_all_args (param i32 i64 f32 f64) (result i32)))
  (import "host" "return_i64" (func $return_i64 (result i64)))
  (import "host" "return_f32" (func $return_f32 (result f32)))
  (import "host" "return_f64" (func $return_f64 (result f64)))
  (import "host" "noop" (func $noop))

  (func (export "call-all-args") (result i32)
    (call $return_i32_from_all_args
      (i32.const -12)
      (i64.const 34)
      (f32.const 5.5)
      (f64.const 6.25)))

  (func (export "call-return-i64") (result i64)
    (call $return_i64))

  (func (export "call-return-f32") (result f32)
    (call $return_f32))

  (func (export "call-return-f64") (result f64)
    (call $return_f64))

  (func (export "call-noop")
    (call $noop))
)

(assert_return (invoke "call-all-args") (i32.const -12))
(assert_return (invoke "call-return-i64") (i64.const 4886718345))
(assert_return (invoke "call-return-f32") (f32.const 12.5))
(assert_return (invoke "call-return-f64") (f64.const 42.25))
(assert_return (invoke "call-noop"))
