default: embedded

out:
	mkdir out

../../crates/spacewasm_c_api/include/spacewasm.h: $(C_CRATE)/src/*
	cargo build -p spacewasm_c_api --release --features codegen

../../target/release/libspacewasm_c_api.a: $(C_CRATE)/src/*
	cargo build -p spacewasm_c_api --release --features codegen

embedded: main.c \
			../../target/release/libspacewasm_c_api.a \
			../../crates/spacewasm_c_api/include/spacewasm.h \
			| out
	clang -nostdlib -nostartfiles -ffreestanding \
			-static \
			-I../../crates/spacewasm_c_api/include \
			-L../../target/release \
			main.c \
			-lspacewasm_c_api \
			-o $@
out/%.bin: | out ../../target/%/release/libspacewasm_c_api.a \
				$(C_CRATE)/include/spacewasm.h \
				main.c \
	

clean:
	rm -rf out
	cargo clean

.PHONY: clean default
.SECONDARY: