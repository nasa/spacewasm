# Security Policy

## Supported Versions

SpaceWasm is currently under pre-release development. Security updates
will be added to the pre-release versions v0.x.x.

| Version | Supported          |
| ------- | ------------------ |
| 0.x.x   | :white_check_mark: |

> [!NOTE]
> Security fixes will _not_ be backported to prior pre-release versions.

## Reporting a Vulnerability

For general bugs, please submit a [Bug Report](https://github.com/nasa/spacewasm/issues).

To report a vulnerability for SpaceWasm, please use the [vulnerability report form](https://github.com/nasa/spacewasm/security/advisories/new).

### Out of Scope

The following are documented, intentional behaviors and will be closed without action:

- **Using an engine after a failed module load.** Any module-load error invalidates the engine, and the embedder must
  drop and reconstruct it. Leftover writes in imported memories or tables, stale table entries, heap poisoning
  (`SPACEWASM_ERR_HEAP_POISONED`), and code-page budget consumed by a rejected module all fall under this contract.
  See the API documentation for [`Module::new`](./src/module.rs) and
  [`spacewasm_load_module`](./crates/spacewasm_c_api/include/spacewasm.h).

If you are using AI tools to assist with your reporting, we require that you disclose this in your report.
Please review the [AI policy](./AI_POLICY.md) for guidelines on disclosure and best practices.
