(module
  (import "regression" "invalid_should_return_some"
    (func $invalid_should_return_some (result i32)))

  (func (export "call") (result i32)
    (call $invalid_should_return_some) unreachable)
)

;; This function should cause a panic because the host function is invalid
(assert_return (invoke "call") (i32.const 0))
