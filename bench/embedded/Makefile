C_CRATE = ../../crates/spacewasm_c_api
RUST_TARGET_OUT = ../../target/release

ARCHS = \
	aarch64-unknown-linux-gnu \
	armv7-unknown-linux-gnueabihf \
	x86_64-unknown-linux-gnu \
	i686-unknown-linux-gnu \
	riscv64gc-unknown-linux-gnu

RUST_LIBS = $(patsubst %,../../target/%/release/libspacewasm_c_api.a,$(ARCHS))

default: $(patsubst %,out/%.bin,$(ARCHS))

out:
	mkdir out

$(C_CRATE)/include/spacewasm.h: $(C_CRATE)/src/*
	cargo build -p spacewasm_c_api --release --features codegen

$(RUST_LIBS): ../../target/%/release/libspacewasm_c_api.a: $(C_CRATE)/src/*
	cargo build -p spacewasm_c_api --release --features codegen --target $*

out/%.bin: | out ../../target/%/release/libspacewasm_c_api.a \
				$(C_CRATE)/include/spacewasm.h \
				main.c \
	clang \
		-static \
		--sysroot=/opt/homebrew/opt/$*/toolchain/$*/sysroot \
		--gcc-toolchain=/opt/homebrew/opt/$*/toolchain \
		-target $* \
		-I$(C_CRATE)/include \
		-L../../target/$*/release \
		main.c \
		-lspacewasm_c_api \
		-o $@

clean:
	rm -rf out
	cargo clean

.PHONY: clean
.SECONDARY: