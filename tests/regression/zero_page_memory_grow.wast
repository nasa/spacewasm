;; Regression for #108: growing a zero-page memory must create usable backing storage.
(module
  (memory 0)
  (func (export "size") (result i32) (memory.size))
  (func (export "grow") (result i32) (memory.grow (i32.const 1)))
  (func (export "load") (result i32) (i32.load8_u (i32.const 0)))
  (func (export "store") (i32.store8 (i32.const 0) (i32.const 165)))
)

(assert_return (invoke "size") (i32.const 0))
(assert_return (invoke "grow") (i32.const 0))
(assert_return (invoke "size") (i32.const 1))
(assert_return (invoke "load") (i32.const 0))
(assert_return (invoke "store"))
(assert_return (invoke "load") (i32.const 165))
