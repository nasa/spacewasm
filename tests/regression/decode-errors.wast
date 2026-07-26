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
