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

/// Which fuzz target's generator produced a seed.
///
/// The `no_traps` and `validate` targets configure wasm-smith differently
/// ([`NoTrapsModule`] sets `disallow_traps`), so they consume input bytes
/// differently and produce different modules from the same seed. A seed must be
/// decoded with the generator that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The `no_traps` target, which uses [`NoTrapsModule`].
    NoTraps,
    /// The `validate` target, which uses [`FuzzModule`].
    Validate,
}

/// Generate the Wasm module a fuzz `target` would produce from `seed`.
///
/// This reproduces what the corresponding fuzz target feeds to its oracle, so a
/// crash artifact can be decoded back into the exact module that triggered it.
pub fn wasm_from_seed(seed: &[u8], target: Target) -> Result<Vec<u8>> {
    let mut u = Unstructured::new(seed);
    match target {
        Target::NoTraps => NoTrapsModule::new(&mut u).map(|m| m.wasm),
        Target::Validate => FuzzModule::new(&mut u).map(|m| m.wasm),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fixed seed with enough bytes for wasm-smith to build a non-trivial module.
    fn seed() -> Vec<u8> {
        (0..=u8::MAX).collect()
    }

    #[test]
    fn targets_produce_different_wasm_from_same_seed() {
        // The two generators configure wasm-smith differently, so the same seed
        // yields different modules -- which is why seed_to_wasm must decode with
        // the generator that produced the seed.
        let no_traps = wasm_from_seed(&seed(), Target::NoTraps).unwrap();
        let validate = wasm_from_seed(&seed(), Target::Validate).unwrap();
        assert_ne!(no_traps, validate);
    }

    #[test]
    fn wasm_from_seed_is_deterministic() {
        assert_eq!(
            wasm_from_seed(&seed(), Target::Validate).unwrap(),
            wasm_from_seed(&seed(), Target::Validate).unwrap(),
        );
    }
}
