;; Chained table imports: owned, re-exported Wasm table, and re-exported host table

(module $At
  (table (export "t") 2 funcref)
  (func $f (result i32) (i32.const 99))
  (elem (i32.const 0) $f)
)
(register "At" $At)

;; Bt re-exports a Wasm table imported from At.
(module $Bt
  (import "At" "t" (table $t 2 funcref))
  (export "t" (table $t))
)
(register "Bt" $Bt)

;; Ct re-exports the host table imported from "spectest".
(module $Ct
  (import "spectest" "table" (table $t 10 funcref))
  (export "t" (table $t))
)
(register "Ct" $Ct)

;; Dt imports the re-exported Wasm table (TableKind::Import resolution path)
;; and calls indirectly through the shared owned table chain.
(module $Dt
  (import "Bt" "t" (table 2 funcref))
  (type $ret_i32 (func (result i32)))
  (func (export "call-0") (result i32)
    (call_indirect (type $ret_i32) (i32.const 0)))
)

;; Et imports the re-exported host table (TableKind::ImportHost resolution path).
;; Wasm 1.0 MVP only allows a single table per module.
(module $Et
  (import "Ct" "t" (table 10 funcref))
)

(assert_return (invoke $Dt "call-0") (i32.const 99))

;; Size mismatch: At."t" has min 2, importing with a larger min is incompatible.
(assert_unlinkable
  (module (import "At" "t" (table 5 funcref)))
  "incompatible import type")

;; Size mismatch against the re-exported Wasm table (TableKind::Import).
(assert_unlinkable
  (module (import "Bt" "t" (table 5 funcref)))
  "incompatible import type")

;; Size mismatch against the re-exported host table (TableKind::ImportHost).
(assert_unlinkable
  (module (import "Ct" "t" (table 100 funcref)))
  "incompatible import type")

;; Size mismatch directly against the host table (min 10, max 20).
(assert_unlinkable
  (module (import "spectest" "table" (table 100 funcref)))
  "incompatible import type")

;; The host module exists but has no table with this field name.
(assert_unlinkable
  (module (import "spectest" "missing-table" (table 1 funcref)))
  "unknown import")

;; The named Wasm export exists but is not a table (a function imported
;; as a table).
(module $Ft
  (func (export "f"))
)
(register "Ft" $Ft)
(assert_unlinkable
  (module (import "Ft" "f" (table 1 funcref)))
  "incompatible import type")

;; Missing field in an existing module.
(assert_unlinkable
  (module (import "At" "missing" (table 1 funcref)))
  "unknown import")
