// Portions of this file are derived from the Wasmtime project
// (https://github.com/bytecodealliance/wasmtime), licensed under
// Apache-2.0 WITH LLVM-exception. These portions have been modified for
// SpaceWasm.

//! Test case generators.
//!
//! Test case generators take raw, unstructured input from a fuzzer
//! (e.g. libFuzzer) and translate that into a structured test case (e.g. a
//! valid Wasm binary).

use arbitrary::{Arbitrary, Result, Unstructured};
use wasm_smith::Config as SmithConfig;

/// Configuration for generating WebAssembly modules for SpaceWasm.
///
/// This wraps wasm-smith's Config with SpaceWasm-specific constraints.
#[derive(Debug, Clone)]
pub struct ModuleConfig {
    config: SmithConfig,
}

impl ModuleConfig {
    /// Create a new module configuration with SpaceWasm constraints.
    pub fn new() -> Self {
        let config = SmithConfig {
            // SpaceWasm constraints
            max_memories: 1,
            max_tables: 1,
            max_table_elements: 1000,
            min_funcs: 0,
            max_funcs: 50,
            min_exports: 0,
            max_exports: 10,
            // Disable imports - SpaceWasm tests don't provide import environment
            min_imports: 0,
            max_imports: 0,
            // Memory limits (SpaceWasm supports max 256 pages = 16MB)
            memory_max_size_required: false,
            max_memory32_bytes: 65536 * 32,
            // Wasm 1.0 MVP compliance - disable all post-MVP features
            allow_start_export: true,
            // Post-MVP proposals - all disabled for MVP compliance
            bulk_memory_enabled: false,
            reference_types_enabled: false,
            simd_enabled: false,
            relaxed_simd_enabled: false,
            exceptions_enabled: false,
            memory64_enabled: false,
            threads_enabled: false,
            multi_value_enabled: false,
            saturating_float_to_int_enabled: false,
            sign_extension_ops_enabled: false,
            tail_call_enabled: false,
            extended_const_enabled: false,
            wide_arithmetic_enabled: false,
            // GC and advanced features
            gc_enabled: false,
            custom_page_sizes_enabled: false,
            shared_everything_threads_enabled: false,
            ..Default::default()
        };

        Self { config }
    }

    /// Generate a WebAssembly module from unstructured input.
    pub fn generate(&self, u: &mut Unstructured<'_>) -> Result<wasm_smith::Module> {
        wasm_smith::Module::new(self.config.clone(), u)
    }

    /// Get the underlying wasm-smith config.
    pub fn smith_config(&self) -> &SmithConfig {
        &self.config
    }

    /// Get a mutable reference to the underlying wasm-smith config.
    pub fn smith_config_mut(&mut self) -> &mut SmithConfig {
        &mut self.config
    }
}

impl Default for ModuleConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Arbitrary<'a> for ModuleConfig {
    fn arbitrary(_u: &mut Unstructured<'a>) -> Result<Self> {
        // Use fixed config to avoid consuming too many input bytes
        // This ensures there's enough data left for wasm-smith module generation
        Ok(ModuleConfig::new())
    }
}

/// A generated WebAssembly module for fuzzing.
#[derive(Debug)]
pub struct FuzzModule {
    /// The Wasm bytes.
    pub wasm: Vec<u8>,
    /// The module configuration used to generate this module.
    pub config: ModuleConfig,
}

impl FuzzModule {
    /// Generate a new fuzz module from unstructured input.
    pub fn new(u: &mut Unstructured<'_>) -> Result<Self> {
        // wasm-smith needs at least a few bytes to generate a minimal module
        // Reject inputs that are too small to avoid panics in wasm-smith
        if u.len() < 4 {
            return Err(arbitrary::Error::NotEnoughData);
        }

        let config = ModuleConfig::arbitrary(u)?;
        let module = config.generate(u)?;
        let wasm = module.to_bytes();

        Ok(Self { wasm, config })
    }

    /// Get the Wasm bytes.
    pub fn wasm(&self) -> &[u8] {
        &self.wasm
    }
}

impl<'a> Arbitrary<'a> for FuzzModule {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        Self::new(u)
    }
}

/// A generated WebAssembly module configured to not trap during execution.
#[derive(Debug)]
pub struct NoTrapsModule {
    /// The Wasm bytes.
    pub wasm: Vec<u8>,
    /// The module configuration used to generate this module.
    pub config: ModuleConfig,
}

impl NoTrapsModule {
    /// Generate a new no-traps module from unstructured input.
    pub fn new(u: &mut Unstructured<'_>) -> Result<Self> {
        // wasm-smith needs at least a few bytes to generate a minimal module
        // Reject inputs that are too small to avoid panics in wasm-smith
        if u.len() < 4 {
            return Err(arbitrary::Error::NotEnoughData);
        }

        let mut config = ModuleConfig::arbitrary(u)?;

        // Configure wasm-smith to generate modules that won't trap
        config.config.disallow_traps = true;

        let module = config.generate(u)?;
        let wasm = module.to_bytes();

        Ok(Self { wasm, config })
    }

    /// Get the Wasm bytes.
    pub fn wasm(&self) -> &[u8] {
        &self.wasm
    }
}

impl<'a> Arbitrary<'a> for NoTrapsModule {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        Self::new(u)
    }
}
