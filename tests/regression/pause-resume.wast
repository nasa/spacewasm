;; Test pause/resume functionality of the interpreter

(module
  (import "regression" "pause_i32" (func $pause_i32 (result i32)))
  (import "regression" "pause_i64" (func $pause_i64 (result i64)))
  (import "regression" "pause_f32" (func $pause_f32 (result f32)))
  (import "regression" "pause_f64" (func $pause_f64 (result f64)))

  ;; Simple pause-resume for i32
  (func (export "test-pause-resume-i32") (result i32)
    (call $pause_i32))

  ;; Dummy functions to provide resume values
  ;; The test harness intercepts these when engine is paused
  (func (export "resume-i32") (param i32) (result i32)
    (local.get 0))

  (func (export "resume-i64") (param i64) (result i64)
    (local.get 0))

  (func (export "resume-f32") (param f32) (result f32)
    (local.get 0))

  (func (export "resume-f64") (param f64) (result f64)
    (local.get 0))

  ;; Simple pause-resume for i64
  (func (export "test-pause-resume-i64") (result i64)
    (call $pause_i64))

  ;; Simple pause-resume for f32
  (func (export "test-pause-resume-f32") (result f32)
    (call $pause_f32))

  ;; Simple pause-resume for f64
  (func (export "test-pause-resume-f64") (result f64)
    (call $pause_f64))

  ;; Pause and use the resumed value in arithmetic
  (func (export "test-pause-i32-arithmetic") (result i32)
    (i32.add
      (call $pause_i32)
      (i32.const 10)))

  ;; Pause and use the resumed value in i64 arithmetic
  (func (export "test-pause-i64-arithmetic") (result i64)
    (i64.mul
      (call $pause_i64)
      (i64.const 2)))

  ;; Pause and use the resumed value in f32 arithmetic
  (func (export "test-pause-f32-arithmetic") (result f32)
    (f32.sub
      (call $pause_f32)
      (f32.const 1.5)))

  ;; Pause and use the resumed value in f64 arithmetic
  (func (export "test-pause-f64-arithmetic") (result f64)
    (f64.div
      (call $pause_f64)
      (f64.const 2.0)))

  ;; Multiple pauses in sequence
  (func (export "test-multiple-pauses-i32") (result i32)
    (i32.add
      (call $pause_i32)
      (call $pause_i32)))

  ;; Mixed type pauses - pause i32 and i64, return i32
  (func (export "test-mixed-pause-types") (result i32)
    (i32.add
      (call $pause_i32)
      (i32.wrap_i64 (call $pause_i64))))

  ;; Nested call with pause
  (func $helper-with-pause (result i32)
    (i32.mul
      (call $pause_i32)
      (i32.const 3)))

  (func (export "test-nested-pause") (result i32)
    (i32.add
      (call $helper-with-pause)
      (i32.const 5)))

  ;; Conditional with pause
  (func (export "test-conditional-pause") (param i32) (result i32)
    (if (result i32) (local.get 0)
      (then (call $pause_i32))
      (else (i32.const 999))))

  ;; Local variable interaction with pause
  (func (export "test-pause-with-locals") (result i32)
    (local $temp i32)
    (local.set $temp (i32.const 5))
    (i32.add
      (local.get $temp)
      (call $pause_i32)))
)

;;  Test Pattern:
;; 1. assert_trap with "paused" - calls the function, it pauses
;; 2. Immediately invoke the resume-* function with the resume value
;;    The test harness detects the paused state and resumes instead of invoking

;; Basic pause/resume tests
(assert_trap (invoke "test-pause-resume-i32") "paused")
(assert_return (invoke "resume-i32" (i32.const 42)) (i32.const 42))

(assert_trap (invoke "test-pause-resume-i64") "paused")
(assert_return (invoke "resume-i64" (i64.const 12345678)) (i64.const 12345678))

(assert_trap (invoke "test-pause-resume-f32") "paused")
(assert_return (invoke "resume-f32" (f32.const 3.14)) (f32.const 3.14))

(assert_trap (invoke "test-pause-resume-f64") "paused")
(assert_return (invoke "resume-f64" (f64.const 2.718)) (f64.const 2.718))

;; Arithmetic with paused values
(assert_trap (invoke "test-pause-i32-arithmetic") "paused")
(assert_return (invoke "resume-i32" (i32.const 42)) (i32.const 52))  ;; 42 + 10

(assert_trap (invoke "test-pause-i64-arithmetic") "paused")
(assert_return (invoke "resume-i64" (i64.const 100)) (i64.const 200)) ;; 100 * 2

(assert_trap (invoke "test-pause-f32-arithmetic") "paused")
(assert_return (invoke "resume-f32" (f32.const 3.0)) (f32.const 1.5)) ;; 3.0 - 1.5

(assert_trap (invoke "test-pause-f64-arithmetic") "paused")
(assert_return (invoke "resume-f64" (f64.const 10.0)) (f64.const 5.0)) ;; 10.0 / 2.0

;; Nested pause preserves call stack
(assert_trap (invoke "test-nested-pause") "paused")
(assert_return (invoke "resume-i32" (i32.const 5)) (i32.const 20)) ;; (5 * 3) + 5

;; Conditional with pause
(assert_trap (invoke "test-conditional-pause" (i32.const 1)) "paused")
(assert_return (invoke "resume-i32" (i32.const 42)) (i32.const 42))

(assert_return (invoke "test-conditional-pause" (i32.const 0)) (i32.const 999))

;; Locals preserved across pause
(assert_trap (invoke "test-pause-with-locals") "paused")
(assert_return (invoke "resume-i32" (i32.const 42)) (i32.const 47)) ;; 5 + 42
