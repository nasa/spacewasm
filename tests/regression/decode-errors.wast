;; Decoder / validator error paths exercised through raw binary encodings.
;;
;; These modules are hand-encoded so that they reach specific malformed-byte
;; branches in the decoder that the upstream spec suite does not cover (the
;; upstream text-format assertions are rejected by `wast2json` before ever
;; reaching our decoder). The expected-error strings are matched against
;; `ValidationError` variants in `tests/util/spectest.rs`.

;; ---------------------------------------------------------------------------
;; Function type entries must begin with the 0x60 leading byte.
;; type section (id 1): count=1, leading byte 0x61 (invalid)
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\01\02\01\61")
  "malformed function type")

;; ---------------------------------------------------------------------------
;; Table element type must be funcref (0x70).
;; table section (id 4): count=1, elem type 0x6f (invalid)
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\04\02\01\6f")
  "malformed element type")

;; ---------------------------------------------------------------------------
;; Limit flag byte must be 0x00 or 0x01.
;; table section (id 4): count=1, funcref (0x70), limit flag 0x02 (invalid)
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\04\03\01\70\02")
  "malformed limits flag")

;; ---------------------------------------------------------------------------
;; A table limit whose maximum is smaller than its minimum is rejected.
;; table section (id 4): count=1, funcref, flag=0x01 (has max), min=2, max=1
;; ---------------------------------------------------------------------------
(assert_invalid
  (module binary "\00asm\01\00\00\00\04\05\01\70\01\02\01")
  "size minimum must not be greater than maximum")

;; ---------------------------------------------------------------------------
;; A table whose `limits.min` is used unbounded as an allocation length is
;; rejected at decode time (symmetric with the memory-size bound). Left
;; unchecked, `min` drove a panic on 32-bit targets (Layout::array failure) or
;; a multi-gigabyte allocation on 64-bit hosts.
;; table section (id 4): count=1, funcref, flag=0x00 (min only), min=0xFFFFFFFF
;; ---------------------------------------------------------------------------
(assert_invalid
  (module binary "\00asm\01\00\00\00\04\08\01\70\00\ff\ff\ff\ff\0f")
  "table size too large")

;; ---------------------------------------------------------------------------
;; Memory type flag with the "shared" bit (bit 1) set is unsupported.
;; memory section (id 5): count=1, flag 0x02, min 0
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\05\03\01\02\00")
  "malformed memory type")

;; ---------------------------------------------------------------------------
;; Memory type flag with a reserved high bit set is unsupported.
;; memory section (id 5): count=1, flag 0x10, min 0
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\05\03\01\10\00")
  "malformed memory type")

;; ---------------------------------------------------------------------------
;; Memory type flag with bit 2 (i64 index type, memory64 proposal) set is
;; unsupported.
;; memory section (id 5): count=1, flag 0x04, min 0
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\05\03\01\04\00")
  "malformed memory type")

;; ---------------------------------------------------------------------------
;; A memory declaring its page size explicitly with the 64KiB exponent (16)
;; decodes to the default page size.
;; memory section (id 5): count=1, flag 0x08 (custom page size, no max),
;; min 1, page-size exponent 16.
;; ---------------------------------------------------------------------------
(module binary "\00asm\01\00\00\00\05\04\01\08\01\10")

;; ---------------------------------------------------------------------------
;; Import descriptor kind byte must be 0x00-0x03.
;; import section (id 2): count=1, module name "", field name "", desc 0x04
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\02\04\01\00\00\04")
  "malformed import kind")

;; ---------------------------------------------------------------------------
;; Export descriptor kind byte must be 0x00-0x03.
;; export section (id 7): count=1, name "", desc 0x04
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\07\03\01\00\04")
  "malformed export kind")

;; ---------------------------------------------------------------------------
;; Global mutability byte must be 0x00 (const) or 0x01 (var).
;; global section (id 6): count=1, valtype i32 (0x7f), mutability 0x02
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\06\03\01\7f\02")
  "malformed mutability")

;; ---------------------------------------------------------------------------
;; Sections must appear in ascending id order.
;; global section (id 6, empty) followed by type section (id 1)
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\06\01\00\01\01\00")
  "unexpected section order")

;; ---------------------------------------------------------------------------
;; Unknown section id (12 is beyond the last defined data section, id 11).
;; ---------------------------------------------------------------------------
(assert_malformed
  (module binary "\00asm\01\00\00\00\0c\01\00")
  "malformed section id")

;; ---------------------------------------------------------------------------
;; A local variable's frame offset is encoded as a signed 16-bit value, but the
;; validator otherwise permits up to 0xFFFF words of locals. A high local index
;; used to wrap the `as i16` cast into a negative offset, producing an
;; out-of-bounds stack read/write at runtime. The offset-encoding site now
;; rejects any local whose word offset cannot be represented.
;; One function declaring 40000 i32 locals (accepted by the size validator)
;; whose body accesses `local.get 35000` (word offset 35000 > i16::MAX - 2).
;; ---------------------------------------------------------------------------
(assert_invalid
  (module binary
    "\00asm\01\00\00\00\01\04\01\60\00\00\03\02\01\00\07\08"
    "\01\04\74\65\73\74\00\00\0a\0d\01\0b\01\c0\b8\02\7f\20\b8\91"
    "\02\1a\0b")
  "local offset out of range")

;; ---------------------------------------------------------------------------
;; A result-typed `if` without an `else` used to be accepted when the then-arm
;; ended unreachable. The false path is still reachable and produces no result,
;; desynchronizing the validator's operand-stack model from the runtime stack
;; pointer. Such a module must be rejected regardless of then-arm reachability.
;; Function `f` of type () -> i32 whose body is
;; `i32.const 0; if (result i32); unreachable; end`.
;; ---------------------------------------------------------------------------
(assert_invalid
  (module binary
    "\00asm\01\00\00\00\01\05\01\60\00\01\7f\03\02\01\00\07"
    "\05\01\01\66\00\00\0a\0a\01\08\00\41\00\04\7f\00\0b\0b")
  "result-typed if without else")
