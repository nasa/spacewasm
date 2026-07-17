;; Maximum memory sizes.

;; i32 (pagesize 1)
(assert_invalid
  (module
    (memory 0xFFFF_FFFF (pagesize 1)))
  "allocation failed")

;; i32 (default pagesize)
(assert_invalid
  (module
    (memory 65536 (pagesize 65536)))
  "allocation failed")

;; Memory size just over the maximum.

;; i32 (default pagesize)
(assert_invalid
  (module
    (memory 65537 (pagesize 65536)))
  "memory size must be at most 4 GiB")
