# Adversarial Testing for SpaceWasm Modules

SpaceWasm is a flight-compliant WebAssembly interpreter.
This doc describes dual-model adversarial test generation.

## Threat Model

Flight WASM modules may receive malformed sensor inputs,
out-of-bounds indices from corrupted telemetry,
or unexpected state transitions from concurrent systems.

## Dual-Model Gate Pattern

```
Generator model  ->  happy-path test cases
     down
Adversarial gate  ->  boundary values, type confusion,
    sequence attacks, resource exhaustion
     down
SpaceWasm  ->  runs all cases under fuel limit
```

## Key Invariants

1. Fuel-bounded execution -- no unbounded runs
2. Memory-safe -- no access outside declared pages
3. Deterministic -- same inputs, same outputs
4. No host imports outside whitelist
