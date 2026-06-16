(assert_invalid
  (module (func $type-arg-num-vs-arg-num
    (block
      (block (result f32)
        (br_table 0 1 (f32.const 0) (i32.const 0))
      )
      (drop)
    )
  ))
  "type mismatch"
)