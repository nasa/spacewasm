;; Chained global imports and re-exported host globals

(module $Ag
  (global $g (export "g") (mut i32) (i32.const 100))
  (func (export "get") (result i32) (global.get $g))
)
(register "Ag" $Ag)

;; Bg re-exports a Wasm global imported from Ag and a mutable host global
;; imported from the "regression" host module.
(module $Bg
  (global $g (import "Ag" "g") (mut i32))
  (global $hg (import "regression" "mut_global_i64") (mut i64))
  (export "g" (global $g))
  (export "hg" (global $hg))
)
(register "Bg" $Bg)

;; Cg imports the *re-exported* globals from Bg.
;;   Bg."g"  resolves to Ref::Extern (an imported Wasm global)
;;   Bg."hg" resolves to Ref::Host   (an imported host global)
(module $Cg
  (global $g (import "Bg" "g") (mut i32))
  (global $hg (import "Bg" "hg") (mut i64))

  (func (export "get") (result i32) (global.get $g))
  (func (export "set") (param i32) (global.set $g (local.get 0)))

  (func (export "get-host") (result i64) (global.get $hg))
  (func (export "set-host") (param i64) (global.set $hg (local.get 0)))
)

(assert_return (invoke $Cg "get") (i32.const 100))
(assert_return (invoke $Cg "set" (i32.const 7)))
(assert_return (invoke $Cg "get") (i32.const 7))
;; The write propagates back through the chain to the owning module Ag.
(assert_return (invoke $Ag "get") (i32.const 7))

(assert_return (invoke $Cg "set-host" (i64.const 55)))
(assert_return (invoke $Cg "get-host") (i64.const 55))

;; Type mismatch against a host global (wrong value type).
(assert_unlinkable
  (module (global (import "regression" "mut_global_i64") (mut i32)))
  "incompatible import type")

;; Mutability mismatch against a host global (host is mutable, import asks
;; for immutable).
(assert_unlinkable
  (module (global (import "regression" "mut_global_i32") i32))
  "incompatible import type")

;; Type mismatch against a Wasm global (Ag."g" is i32, import asks for i64).
(assert_unlinkable
  (module (global (import "Ag" "g") (mut i64)))
  "incompatible import type")

;; Mutability mismatch against a Wasm global (Ag."g" is mutable, import asks
;; for immutable) — the Ref::Module branch.
(assert_unlinkable
  (module (global (import "Ag" "g") i32))
  "incompatible import type")

;; The named export exists but is not a global (a function imported as a global).
(assert_unlinkable
  (module (global (import "Ag" "get") i32))
  "incompatible import type")

;; The module exists but the field does not.
(assert_unlinkable
  (module (global (import "Ag" "missing") i32))
  "unknown import")

;; Type mismatch against the re-exported Ref::Extern global.
(assert_unlinkable
  (module (global (import "Bg" "g") (mut i64)))
  "incompatible import type")

;; Type mismatch against the re-exported Ref::Host global.
(assert_unlinkable
  (module (global (import "Bg" "hg") (mut i32)))
  "incompatible import type")

;; Mutability mismatch against the re-exported Ref::Extern global.
(assert_unlinkable
  (module (global (import "Bg" "g") i32))
  "incompatible import type")

;; Mutability mismatch against the re-exported Ref::Host global.
(assert_unlinkable
  (module (global (import "Bg" "hg") i64))
  "incompatible import type")
