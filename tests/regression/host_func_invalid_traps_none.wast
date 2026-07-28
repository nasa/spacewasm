(module
  (import "regression" "invalid_should_return_none"
    (func $invalid_should_return_none))

  (func (export "call") (result i32)
    (call $invalid_should_return_none) unreachable)
)

;; The host function returns a value despite its declared empty result type:
;; the call traps (host signature mismatch) instead of aborting the process.
(assert_trap (invoke "call") "host function signature mismatch")
