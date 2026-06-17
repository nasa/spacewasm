//! Test case generators.
//!
//! Test case generators take raw, unstructured input from a fuzzer
//! (e.g. libFuzzer) and translate that into a structured test case (e.g. a
//! valid Wasm binary).

use arbitrary::{Arbitrary, Result, Unstructured};
use wasm_smith::Config as SmithConfig;

/// Configuration for generating WebAssembly modules for SpaceWASM.
///
/// This wraps wasm-smith's Config with SpaceWASM-specific constraints.
#[derive(Debug, Clone)]
pub struct ModuleConfig {
    config: SmithConfig,
}

impl ModuleConfig {
    /// Create a new module configuration with SpaceWASM constraints.
    pub fn new() -> Self {
        let mut config = SmithConfig::default();

        // SpaceWASM constraints
        config.max_memories = 1;
        config.max_tables = 1;
        config.max_table_elements = 1000;
        config.min_funcs = 0;
        config.max_funcs = 50;
        config.min_exports = 0;
        config.max_exports = 10;

        // Disable imports - SpaceWASM tests don't provide import environment
        config.min_imports = 0;
        config.max_imports = 0;

        // Memory limits (SpaceWASM supports max 256 pages = 16MB)
        // Note: These are configured via max_memory_pages in MemoryConfig
        config.memory_max_size_required = false;
        config.max_memory32_bytes = 65536 * 32;

        // WASM 1.0 MVP compliance - disable all post-MVP features
        config.allow_start_export = true;

        // Post-MVP proposals - all disabled for MVP compliance
        config.bulk_memory_enabled = false;
        config.reference_types_enabled = false;
        config.simd_enabled = false;
        config.relaxed_simd_enabled = false;
        config.exceptions_enabled = false;
        config.memory64_enabled = false;
        config.threads_enabled = false;
        config.multi_value_enabled = false;
        config.saturating_float_to_int_enabled = false;
        config.sign_extension_ops_enabled = false;
        config.tail_call_enabled = false;
        config.extended_const_enabled = false;
        config.wide_arithmetic_enabled = false;

        // GC and advanced features
        config.gc_enabled = false;
        config.custom_page_sizes_enabled = false;
        config.shared_everything_threads_enabled = false;

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
    /// The WASM bytes.
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

    /// Get the WASM bytes.
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
    /// The WASM bytes.
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

    /// Get the WASM bytes.
    pub fn wasm(&self) -> &[u8] {
        &self.wasm
    }
}

impl<'a> Arbitrary<'a> for NoTrapsModule {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        Self::new(u)
    }
}
