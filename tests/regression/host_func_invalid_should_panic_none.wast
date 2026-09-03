(module
  (import "regression" "invalid_should_return_none"
    (func $invalid_should_return_none))

  (func (export "call") (result i32)
    (call $invalid_should_return_none) unreachable)
)

;; Invoking "call" panics because the host function is invalid, so the
;; assert_return below is never reached. The panic is the actual pass
;; criterion, asserted by the #[should_panic] attribute on
;; host_func_invalid_should_panic_none in tests/regression_integration.rs.
(assert_return (invoke "call") (i32.const 0))
