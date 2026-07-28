(module
  (import "regression" "invalid_should_return_some"
    (func $invalid_should_return_some (result i32)))

  (func (export "call") (result i32)
    (call $invalid_should_return_some) unreachable)
)

;; The host function returns nothing despite its declared i32 result type:
;; the call traps (host signature mismatch) instead of aborting the process.
(assert_trap (invoke "call") "host function signature mismatch")
