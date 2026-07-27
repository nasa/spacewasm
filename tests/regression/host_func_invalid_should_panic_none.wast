(module
  (import "regression" "invalid_should_return_none"
    (func $invalid_should_return_none))

  (func (export "call") (result i32)
    (call $invalid_should_return_none) unreachable)
)

;; This function should cause a panic because the host function is invalid
(assert_return (invoke "call") (i32.const 0))
