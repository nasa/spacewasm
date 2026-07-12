;; External globals imported from another Wasm module

(module $Mg
  (global $i32 (export "i32") (mut i32) (i32.const 10))
  (global $i64 (export "i64") (mut i64) (i64.const 20))
  (global $f32 (export "f32") (mut f32) (f32.const 30.5))
  (global $f64 (export "f64") (mut f64) (f64.const 40.25))

  (func (export "get-i32") (result i32) (global.get $i32))
  (func (export "get-i64") (result i64) (global.get $i64))
  (func (export "get-f32") (result f32) (global.get $f32))
  (func (export "get-f64") (result f64) (global.get $f64))
)

(register "Mg" $Mg)

(module $Ng
  (global $i32 (import "Mg" "i32") (mut i32))
  (global $i64 (import "Mg" "i64") (mut i64))
  (global $f32 (import "Mg" "f32") (mut f32))
  (global $f64 (import "Mg" "f64") (mut f64))

  (func (export "read-i32") (result i32) (global.get $i32))
  (func (export "write-i32") (param i32) (global.set $i32 (local.get 0)))

  (func (export "read-i64") (result i64) (global.get $i64))
  (func (export "write-i64") (param i64) (global.set $i64 (local.get 0)))

  (func (export "read-f32") (result f32) (global.get $f32))
  (func (export "write-f32") (param f32) (global.set $f32 (local.get 0)))

  (func (export "read-f64") (result f64) (global.get $f64))
  (func (export "write-f64") (param f64) (global.set $f64 (local.get 0)))
)

(assert_return (invoke $Ng "read-i32") (i32.const 10))
(assert_return (invoke $Ng "read-i64") (i64.const 20))
(assert_return (invoke $Ng "read-f32") (f32.const 30.5))
(assert_return (invoke $Ng "read-f64") (f64.const 40.25))

(assert_return (invoke $Ng "write-i32" (i32.const 11)))
(assert_return (invoke $Ng "read-i32") (i32.const 11))
(assert_return (invoke $Mg "get-i32") (i32.const 11))

(assert_return (invoke $Ng "write-i64" (i64.const 22)))
(assert_return (invoke $Ng "read-i64") (i64.const 22))
(assert_return (invoke $Mg "get-i64") (i64.const 22))

(assert_return (invoke $Ng "write-f32" (f32.const 33.5)))
(assert_return (invoke $Ng "read-f32") (f32.const 33.5))
(assert_return (invoke $Mg "get-f32") (f32.const 33.5))

(assert_return (invoke $Ng "write-f64" (f64.const 44.25)))
(assert_return (invoke $Ng "read-f64") (f64.const 44.25))
(assert_return (invoke $Mg "get-f64") (f64.const 44.25))
