;; Chained function imports across Wasm modules and re-exported host functions

(module $Af
  (func (export "add") (param i32 i32) (result i32)
    (i32.add (local.get 0) (local.get 1)))
  (global (export "g") i32 (i32.const 0))
)
(register "Af" $Af)

;; Bf re-exports a Wasm function imported from Af and a host function imported
;; from the "regression" host module.
(module $Bf
  (import "Af" "add" (func $add (param i32 i32) (result i32)))
  (import "regression" "return_i64" (func $ret64 (result i64)))
  (export "add" (func $add))
  (export "ret64" (func $ret64))
)
(register "Bf" $Bf)

;; Cf imports the *re-exported* functions from Bf.
;;   Bf."add"   resolves to Ref::Extern (an imported Wasm function)
;;   Bf."ret64" resolves to Ref::Host   (an imported host function)
(module $Cf
  (import "Bf" "add" (func $add (param i32 i32) (result i32)))
  (import "Bf" "ret64" (func $ret64 (result i64)))

  (func (export "call-add") (result i32)
    (call $add (i32.const 20) (i32.const 22)))
  (func (export "call-ret64") (result i64)
    (call $ret64))
)

(assert_return (invoke $Cf "call-add") (i32.const 42))
(assert_return (invoke $Cf "call-ret64") (i64.const 4886718345))

;; Signature mismatches against a host function, a local Wasm function
;; re-exported through Ref::Extern, and a re-exported host function.
(assert_unlinkable
  (module (import "regression" "return_i64" (func (result i32))))
  "incompatible import type")

(assert_unlinkable
  (module (import "Bf" "add" (func (result i32))))
  "incompatible import type")

(assert_unlinkable
  (module (import "Bf" "ret64" (func (param i32))))
  "incompatible import type")

;; Signature mismatch against a local Wasm function (Ref::Module path).
(assert_unlinkable
  (module (import "Af" "add" (func (result i32))))
  "incompatible import type")

;; The named export exists but is not a function (a global imported as a func).
(assert_unlinkable
  (module (import "Af" "g" (func)))
  "incompatible import type")

;; Imports that name a module/field which does not exist.
(assert_unlinkable
  (module (import "Bf" "missing" (func)))
  "unknown import")
