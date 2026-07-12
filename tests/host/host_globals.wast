;; Host global reads and writes

(module
  (import "spectest" "global_i32" (global $global_i32 i32))
  (import "spectest" "global_i64" (global $global_i64 i64))
  (import "spectest" "global_f32" (global $global_f32 f32))
  (import "spectest" "global_f64" (global $global_f64 f64))

  (import "spectest" "mut_global_i32" (global $mut_global_i32 (mut i32)))
  (import "spectest" "mut_global_i64" (global $mut_global_i64 (mut i64)))
  (import "spectest" "mut_global_f32" (global $mut_global_f32 (mut f32)))
  (import "spectest" "mut_global_f64" (global $mut_global_f64 (mut f64)))

  (func (export "read-i32") (result i32) (global.get $global_i32))
  (func (export "read-i64") (result i64) (global.get $global_i64))
  (func (export "read-f32") (result f32) (global.get $global_f32))
  (func (export "read-f64") (result f64) (global.get $global_f64))

  (func (export "set-i32") (param i32)
    (global.set $mut_global_i32 (local.get 0)))
  (func (export "get-i32") (result i32) (global.get $mut_global_i32))

  (func (export "set-i64") (param i64)
    (global.set $mut_global_i64 (local.get 0)))
  (func (export "get-i64") (result i64) (global.get $mut_global_i64))

  (func (export "set-f32") (param f32)
    (global.set $mut_global_f32 (local.get 0)))
  (func (export "get-f32") (result f32) (global.get $mut_global_f32))

  (func (export "set-f64") (param f64)
    (global.set $mut_global_f64 (local.get 0)))
  (func (export "get-f64") (result f64) (global.get $mut_global_f64))
)

(assert_return (invoke "read-i32") (i32.const 666))
(assert_return (invoke "read-i64") (i64.const 666))
(assert_return (invoke "read-f32") (f32.const 666.6))
(assert_return (invoke "read-f64") (f64.const 666.6))

(assert_return (invoke "set-i32" (i32.const 11)))
(assert_return (invoke "get-i32") (i32.const 11))

(assert_return (invoke "set-i64" (i64.const 22)))
(assert_return (invoke "get-i64") (i64.const 22))

(assert_return (invoke "set-f32" (f32.const 33.5)))
(assert_return (invoke "get-f32") (f32.const 33.5))

(assert_return (invoke "set-f64" (f64.const 44.25)))
(assert_return (invoke "get-f64") (f64.const 44.25))
