;; Maximum memory sizes.

;; i32 (pagesize 1)
(module
  (memory 0xFFFF_FFFF (pagesize 1)))

;; i32 (default pagesize)
(module
  (memory 65536 (pagesize 65536)))

;; Memory size just over the maximum.

;; i32 (default pagesize)
(assert_invalid
  (module
    (memory 65537 (pagesize 65536)))
  "memory size must be at most 4 GiB")
