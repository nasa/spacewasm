;; A module that initializes an imported host table through an active element
;; segment, where the host still holds the table's backing `Rc`.
;;
;; During element-segment initialization `Module::get_table` takes a unique
;; borrow of the table's `Rc`. When the `Rc` is aliased (here: the host module
;; exposes the same table under two symbol names, so its strong count is
;; permanently >= 2), that borrow fails and the load must be rejected with
;; `ValidationError::TableRefNotUnique` instead of panicking. This is a
;; SpaceWasm-specific rejection with no upstream-spec equivalent, so the
;; standard suite never covers it.
;;
;; The host module `alias` is defined by `aliased_table_host_module()` in
;; `tests/regression_integration.rs`; the "table reference not unique" string is
;; matched against the error variant in `tests/util/spectest.rs`.
;;
;; ```wat
;; (module
;;   (import "alias" "t" (table 4 funcref))
;;   (type $t (func (result i32)))
;;   (func $f (type $t) (i32.const 7))
;;   (elem (i32.const 0) $f))
;; ```
(assert_invalid
  (module binary "\00asm\01\00\00\00\01\05\01\60\00\01\7f\02\0d\01\05alias\01t\01\70\00\04\03\02\01\00\09\07\01\00\41\00\0b\01\00\0a\06\01\04\00\41\07\0b")
  "table reference not unique")
