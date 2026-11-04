;; Regression for data segment offsets at or above 2 GiB, found by Wasmtime's
;; `misc_testsuite/big-memory-behavior.wast`. The offset is an unsigned `i32`,
;; so an offset that is negative when read as signed is still in bounds of a
;; memory larger than 2 GiB.

;; 0x8001 pages is 2 GiB + 64 KiB.
(module
  (memory 0x8001)
  (data (i32.const 0x8000_0000) "x")
  (data (i32.const 0x8000_ffff) "y")
  (func (export "load") (param i32) (result i32)
    (i32.load8_u (local.get 0))))

(assert_return (invoke "load" (i32.const 0x8000_0000)) (i32.const 0x78))
(assert_return (invoke "load" (i32.const 0x8000_ffff)) (i32.const 0x79))
(assert_return (invoke "load" (i32.const 0x7fff_ffff)) (i32.const 0))

;; Offsets past the end of the memory are still rejected.
(assert_unlinkable
  (module
    (memory 0x8001)
    (data (i32.const 0x8001_0000) "x"))
  "data segment does not fit")

(assert_unlinkable
  (module
    (memory 0x8001)
    (data (i32.const 0x8000_ffff) "xy"))
  "data segment does not fit")
