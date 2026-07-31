C_CRATE = ../../crates/spacewasm_c_api
RUST_TARGET_OUT = ../../target/release

ARCHS = \
	aarch64-unknown-linux-gnu \
	armv7-unknown-linux-gnueabihf \
	x86_64-unknown-linux-gnu \
	i686-unknown-linux-gnu \
# 	riscv64gc-unknown-linux-gnu

RUST_LIBS = $(patsubst %,../../target/%/release/libspacewasm_c_api.a,$(ARCHS))

default: $(patsubst %,out/initramfs-%.cpio.gz,$(ARCHS))

out:
	mkdir out

out/coremark_wasm.h: coremark.wasm | out
	xxd -i -name COREMARK_WASM coremark.wasm > out/coremark_wasm.h

$(C_CRATE)/include/spacewasm.h: $(C_CRATE)/src/*
	cargo build -p spacewasm_c_api --release --features codegen

$(RUST_LIBS): ../../target/%/release/libspacewasm_c_api.a: $(C_CRATE)/src/*
	cargo build -p spacewasm_c_api --release --features codegen --target $*

out/coremark-%: ../../target/%/release/libspacewasm_c_api.a \
				$(C_CRATE)/include/spacewasm.h \
				coremark.c \
				out/coremark_wasm.h \
				| out
	clang \
		-static \
		-O3 \
		--sysroot=/opt/homebrew/opt/$*/toolchain/$*/sysroot \
		--gcc-toolchain=/opt/homebrew/opt/$*/toolchain \
		-target $* \
		-I$(C_CRATE)/include \
		-L../../target/$*/release \
		coremark.c \
		-lspacewasm_c_api \
		-o $@

out/initramfs-%.cpio.gz: out/coremark-%
	mkdir -p out/root-$*/{bin,sbin,etc,proc,sys,dev,usr/bin,usr/sbin}
	cp out/coremark-$* out/root-$*/init
	chmod +x out/root-$*/init
	cd out/root-$* && \
		find . -print0 | cpio --null --create --format=newc | gzip -9 > ../initramfs-$*.cpio.gz

download-kernels: \
	out/vmlinuz_aarch64_netboot-3.21.7 	\
	out/vmlinuz_armv7_netboot-3.21.7 	\
	out/vmlinuz_x86_64_netboot-3.21.7 	\
	out/vmlinuz_x86_netboot-3.21.7 		\
	| out
# 	out/vmlinuz_riscv64_netboot-3.21.7

out/vmlinuz_aarch64_netboot-3.21.7:
	curl https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/aarch64/netboot-3.21.7/vmlinuz-virt > out/vmlinuz_aarch64_netboot-3.21.7
out/vmlinuz_armv7_netboot-3.21.7:
	curl https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/armv7/netboot-3.21.7/vmlinuz-lts > out/vmlinuz_armv7_netboot-3.21.7
out/vmlinuz_x86_64_netboot-3.21.7:
	curl https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot-3.21.7/vmlinuz-lts > out/vmlinuz_x86_64_netboot-3.21.7
out/vmlinuz_x86_netboot-3.21.7:
	curl https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86/netboot-3.21.7/vmlinuz-lts > out/vmlinuz_x86_netboot-3.21.7
out/vmlinuz_riscv64_netboot-3.21.7:
	curl https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/riscv64/netboot-3.21.7/vmlinuz-lts > out/vmlinuz_riscv64_netboot-3.21.7

clean:
	rm -rf out
	cargo clean

.PHONY: clean download-kernels
.SECONDARY: