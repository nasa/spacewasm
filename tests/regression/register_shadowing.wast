;; Regression for re-registering a module name, found by Wasmtime's
;; `misc_testsuite/module-interposition.wast`. A later `register` under an
;; existing name must shadow the earlier module for subsequent imports, rather
;; than imports continuing to resolve to the first module registered.

;; Each module imports `"instance" "export"` and is then registered under the
;; same name, so every call goes through the whole chain: 7 * 2^4 = 112.
(module $m0 (func (export "export") (result i32) (i32.const 7)))
(register "instance" $m0)

(module $m1
  (import "instance" "export" (func (result i32)))
  (func (export "export") (result i32) (i32.mul (call 0) (i32.const 2))))
(register "instance" $m1)

(module $m2
  (import "instance" "export" (func (result i32)))
  (func (export "export") (result i32) (i32.mul (call 0) (i32.const 2))))
(register "instance" $m2)

(module $m3
  (import "instance" "export" (func (result i32)))
  (func (export "export") (result i32) (i32.mul (call 0) (i32.const 2))))
(register "instance" $m3)

(module
  (import "instance" "export" (func (result i32)))
  (func (export "export") (result i32) (i32.mul (call 0) (i32.const 2))))

(assert_return (invoke "export") (i32.const 112))

;; Shadowed modules remain reachable by their `$name`, and links made while
;; they were registered keep pointing at them.
(assert_return (invoke $m0 "export") (i32.const 7))
(assert_return (invoke $m1 "export") (i32.const 14))
(assert_return (invoke $m2 "export") (i32.const 28))
(assert_return (invoke $m3 "export") (i32.const 56))

;; Shadowing also applies to globals: the newest registration wins.
(module $g0 (global (export "g") i32 (i32.const 1)))
(register "globals" $g0)
(module $g1 (global (export "g") i32 (i32.const 2)))
(register "globals" $g1)

(module
  (import "globals" "g" (global i32))
  (func (export "get") (result i32) (global.get 0)))

(assert_return (invoke "get") (i32.const 2))

;; Re-registering the same module under its current name is a no-op.
(register "globals" $g1)
(module
  (import "globals" "g" (global i32))
  (func (export "get") (result i32) (global.get 0)))

(assert_return (invoke "get") (i32.const 2))
