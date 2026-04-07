Copyright 2026, by the California Institute of Technology. ALL RIGHTS RESERVED. United States Government Sponsorship acknowledged. Any commercial use must be negotiated with the Office of Technology Transfer at the California Institute of Technology.

This software may be subject to U.S. export control laws. By accepting this software, the user agrees to comply with all applicable U.S. export laws and regulations. User has the responsibility to obtain export licenses, or other export authority as may be required before exporting such information to foreign countries or providing access to foreign persons.

# SpaceWASM

SpaceWASM is an implementation of the [WASM 3.0](https://webassembly.github.io/spec/core/) specification
meant to interpret WASM binary on-board spacecraft. This software comes with two major components:

1. Interpreter -- Meant to be linked into on-board software to interpret and execute WASM binary
2. Embedding interface -- A recommended set of APIs meant to interface between the WASM runtime and host system
