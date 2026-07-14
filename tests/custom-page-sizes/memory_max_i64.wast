;; Maximum memory sizes.

;; i64 (pagesize 1)
(module definition
  (import "test" "unknown" (func))
  (memory i64 0xFFFF_FFFF_FFFF_FFFF (pagesize 1)))

;; i64 (pagesize 1)
(module definition
  (import "test" "unknown" (memory i64 0xFFFF_FFFF_FFFF_FFFF (pagesize 1))))

;; i64 (default pagesize)
(module definition
  (import "test" "unknown" (func))
  (memory i64 0x1_0000_0000_0000 (pagesize 65536)))

;; i64 (default pagesize)
(module definition
  (import "test" "unknown" (memory i64 0x1_0000_0000_0000 (pagesize 65536))))

;; Memory size just over the maximum.
;;
;; These are malformed (for pagesize 1)
;; or invalid (for other pagesizes).

;; i64 (pagesize 1)
(assert_malformed
  (module quote "(memory i64 0x1_0000_0000_0000_0000 (pagesize 1))")
  "i64 constant out of range")

;; i64 (default pagesize)
(assert_invalid
  (module
    (memory i64 0x1_0000_0000_0001 (pagesize 65536)))
  "memory size must be at most")
