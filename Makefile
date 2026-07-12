.PHONY: help fuzz seed-to-wasm trace-wasm trace trace-debug clean-artifacts install-tools

# Default target
help:
	@echo "SpaceWasm Fuzzing Targets"
	@echo ""
	@echo "Fuzzing:"
	@echo "  make fuzz                          Run the no_traps fuzzer"
	@echo "  make fuzz-validate                 Run the validate fuzzer"
	@echo ""
	@echo "Crash Analysis:"
	@echo "  make trace CRASH=<file>            Convert seed + trace (release)"
	@echo "  make trace-debug CRASH=<file>      Convert seed + trace (debug with ASAN)"
	@echo "  make seed-to-wasm CRASH=<file>     Convert fuzzer seed to Wasm"
	@echo "  make trace-wasm WASM=<file>        Trace Wasm file (release)"
	@echo "  make trace-wasm-debug WASM=<file>  Trace Wasm file (debug with ASAN)"
	@echo ""
	@echo "Examples:"
	@echo "  make trace CRASH=fuzz/artifacts/no_traps/crash-abc123"
	@echo "  make trace-debug CRASH=crash-abc123 LIMIT=100"
	@echo "  make trace-wasm WASM=output.wasm LIMIT=50"
	@echo ""
	@echo "Utilities:"
	@echo "  make install-tools                 Install binaries to ~/.cargo/bin"
	@echo "  make clean-artifacts               Delete all fuzzer artifacts"

# Run fuzzer
fuzz:
	cargo +nightly fuzz run no_traps

fuzz-validate:
	cargo +nightly fuzz run validate

# Convert seed to Wasm and trace execution (release mode)
trace:
	@if [ -z "$(CRASH)" ]; then \
		echo "Error: CRASH variable required"; \
		echo "Usage: make trace CRASH=fuzz/artifacts/no_traps/crash-xxx"; \
		exit 1; \
	fi
	@echo "Converting seed to Wasm and tracing execution (release mode)..."
	@cargo run --release -p spacewasm-fuzzing --bin seed_to_wasm -- $(if $(TARGET),--target $(TARGET)) $(CRASH) --stdout 2>/dev/null | \
		cargo run --release -p spacewasm_util --bin spacewasm-trace -- --stdin --limit $(or $(LIMIT),50)

# Convert seed to Wasm and trace execution (debug mode with ASAN)
trace-debug:
	@if [ -z "$(CRASH)" ]; then \
		echo "Error: CRASH variable required"; \
		echo "Usage: make trace-debug CRASH=fuzz/artifacts/no_traps/crash-xxx"; \
		exit 1; \
	fi
	@echo "Converting seed to Wasm and tracing execution (debug mode with ASAN)..."
	@cargo run -p spacewasm-fuzzing --bin seed_to_wasm -- $(if $(TARGET),--target $(TARGET)) $(CRASH) --stdout 2>/dev/null | \
		RUSTFLAGS="-Zsanitizer=address" cargo +nightly run -p spacewasm_util --bin spacewasm-trace -- --stdin --limit $(or $(LIMIT),50)

# Convert fuzzer seed to Wasm
seed-to-wasm:
	@if [ -z "$(CRASH)" ]; then \
		echo "Error: CRASH variable required"; \
		echo "Usage: make seed-to-wasm CRASH=fuzz/artifacts/no_traps/crash-xxx [OUT=output.wasm]"; \
		exit 1; \
	fi
	@if [ -n "$(OUT)" ]; then \
		cargo run --release -p spacewasm-fuzzing --bin seed_to_wasm -- $(if $(TARGET),--target $(TARGET)) $(CRASH) $(OUT); \
	else \
		cargo run --release -p spacewasm-fuzzing --bin seed_to_wasm -- $(if $(TARGET),--target $(TARGET)) $(CRASH) $(CRASH).wasm; \
	fi

# Trace Wasm file execution (release mode)
trace-wasm:
	@if [ -z "$(WASM)" ]; then \
		echo "Error: WASM variable required"; \
		echo "Usage: make trace-wasm WASM=file.wasm [LIMIT=50]"; \
		exit 1; \
	fi
	cargo run --release -p spacewasm_util --bin spacewasm-trace -- $(WASM) --limit $(or $(LIMIT),200)

# Trace Wasm file execution (debug mode with ASAN)
trace-wasm-debug:
	@if [ -z "$(WASM)" ]; then \
		echo "Error: WASM variable required"; \
		echo "Usage: make trace-wasm-debug WASM=file.wasm [LIMIT=50]"; \
		exit 1; \
	fi
	RUSTFLAGS="-Zsanitizer=address" cargo run -p spacewasm_util --bin spacewasm-trace -- $(WASM) --limit $(or $(LIMIT),200)

# Clean fuzzer artifacts
clean-artifacts:
	rm -rf fuzz/artifacts/*
	@echo "Cleaned fuzzer artifacts"
