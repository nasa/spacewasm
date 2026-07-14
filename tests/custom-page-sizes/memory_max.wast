;; Maximum memory sizes.

;; i32 (pagesize 1)
(module
  (memory 0xFFFF_FFFF (pagesize 1)))

;; i32 (pagesize 1)
(module
  (import "a" "b" (memory 0xFFFF_FFFF (pagesize 1))))

;; i32 (default pagesize)
(module
  (import "test" "unknown" (func))
  (memory 65536 (pagesize 65536)))

;; i32 (default pagesize)
(module
  (import "test" "unknown" (memory 65536 (pagesize 65536))))

;; Memory size just over the maximum.

;; i32 (pagesize 1)
(assert_invalid
  (module
    (memory 4294967296 (pagesize 1)))
  "memory size must be at most")

;; i32 (default pagesize)
(assert_invalid
  (module
    (memory 65537 (pagesize 65536)))
  "memory size must be at most")
