;; Chained memory imports: owned, re-exported Wasm memory, and re-exported host memory

(module $Am
  (memory (export "m") 1 2)
  (data (i32.const 0) "\2a\00\00\00")
)
(register "Am" $Am)

;; Bm re-exports a Wasm memory imported from Am.
(module $Bm
  (import "Am" "m" (memory $m 1 2))
  (export "m" (memory $m))
)
(register "Bm" $Bm)

;; Cm re-exports the host memory imported from "spectest" (min 1, max 2).
(module $Cm
  (import "spectest" "memory" (memory $m 1 2))
  (export "m" (memory $m))
)
(register "Cm" $Cm)

;; Dm imports the re-exported Wasm memory, exercising MemoryKind::Import.
(module $Dm
  (import "Bm" "m" (memory 1 2))
  (func (export "read") (result i32)
    (i32.load (i32.const 0)))
  (func (export "write") (param i32)
    (i32.store (i32.const 0) (local.get 0)))
)

;; Em imports the re-exported host memory, exercising MemoryKind::ImportHost.
(module $Em
  (import "Cm" "m" (memory 1 2))
  (func (export "write-host") (param i32)
    (i32.store (i32.const 16) (local.get 0)))
  (func (export "read-host") (result i32)
    (i32.load (i32.const 16)))
)

;; Reads the value placed by Am's data segment, through the import chain.
(assert_return (invoke $Dm "read") (i32.const 42))
;; A write propagates back to the shared owned memory in Am.
(assert_return (invoke $Dm "write" (i32.const 123)))
(assert_return (invoke $Dm "read") (i32.const 123))

(assert_return (invoke $Em "write-host" (i32.const 77)))
(assert_return (invoke $Em "read-host") (i32.const 77))

;; Size mismatch: Am."m" has min 1, importing with a larger min cannot be satisfied.
(assert_unlinkable
  (module (import "Am" "m" (memory 3 4)))
  "incompatible import type")

;; Size mismatch against the re-exported Wasm memory (MemoryKind::Import).
(assert_unlinkable
  (module (import "Bm" "m" (memory 3 4)))
  "incompatible import type")

;; Size mismatch against the re-exported host memory (MemoryKind::ImportHost).
(assert_unlinkable
  (module (import "Cm" "m" (memory 3 4)))
  "incompatible import type")

;; Size mismatch directly against the host memory (min 1, max 2).
(assert_unlinkable
  (module (import "spectest" "memory" (memory 3 4)))
  "incompatible import type")

;; The named Wasm export exists but is not a memory (a function imported
;; as a memory).
(module $Fm
  (func (export "f"))
)
(register "Fm" $Fm)
(assert_unlinkable
  (module (import "Fm" "f" (memory 1)))
  "incompatible import type")

;; Missing field in an existing module.
(assert_unlinkable
  (module (import "Am" "missing" (memory 1)))
  "unknown import")
