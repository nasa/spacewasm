;; LEB128 encodings of the 0xFC-prefixed trunc_sat sub-opcodes from the
;; WebAssembly spec (test/core/binary-leb128.wast @ 340650f2f).

(module binary
  "\00asm" "\01\00\00\00"
  "\01\04\01"                          ;; type section
  "\60\00\00"                          ;; empty function type
  "\03\02\01"                          ;; function section
  "\00"                                ;; function 0, type 0
  "\0a\1b\01\19"                       ;; code section
  "\00"                                ;; no locals
  "\00"                                ;; unreachable
  "\fc\80\00"                          ;; i32_trunc_sat_f32_s with 2 bytes
  "\00"                                ;; unreachable
  "\fc\81\80\00"                       ;; i32_trunc_sat_f32_u with 3 bytes
  "\00"                                ;; unreachable
  "\fc\86\80\80\00"                    ;; i64_trunc_sat_f64_s with 4 bytes
  "\00"                                ;; unreachable
  "\fc\87\80\80\80\00"                 ;; i64_trunc_sat_f64_u with 5 bytes
  "\00"                                ;; unreachable
  "\0b"                                ;; end
)

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01"                          ;; type section
    "\60\00\00"                          ;; empty function type
    "\03\02\01"                          ;; function section
    "\00"                                ;; function 0, type 0
    "\0a\0d\01\0b"                       ;; code section
    "\00"                                ;; no locals
    "\00"                                ;; unreachable
    "\fc\87\80\80\80\80\00"              ;; i64_trunc_sat_f64_u with 6 bytes
    "\00"                                ;; unreachable
    "\0b"                                ;; end
  )
  "integer representation too long"
)
