//! Host modules for a replay, made up from what the module imports.
//!
//! A host call compiles to the same fixed-width instruction whichever host function it
//! names (`CALL_HOST`, the module in the low byte, the function in the next word), so the
//! IR layout only depends on each import resolving to *a* host function of the right
//! signature. A stub that would trap if called is enough: the replay never runs.
//!
//! This only reproduces a store whose imports all resolved to host modules. An import
//! another guest module satisfied compiles to a different instruction, and is refused.

use crate::replay::ReplayError;
use spacewasm::{
    GlobalValue, GlobalValueError, HostFunction, HostFunctionBreak, HostGlobal, HostModule,
    HostValList, ValType, Value,
};
use std::ops::ControlFlow;
use wasmparser::{CompositeInnerType, Parser, Payload, TypeRef};

/// One stub host module per distinct import module name, in first-import order.
pub fn host_modules_for(module: &[u8]) -> Result<spacewasm::Vec<HostModule>, ReplayError> {
    let mut signatures: Vec<(String, String)> = Vec::new();
    let mut modules: Vec<(String, Vec<HostFunction>, Vec<HostGlobal>)> = Vec::new();

    let refuse = |why: String| ReplayError::Rejected {
        offset: 0,
        reason: why,
    };

    for payload in Parser::new(0).parse_all(module) {
        match payload.map_err(|err| refuse(err.to_string()))? {
            Payload::TypeSection(reader) => {
                for group in reader {
                    for ty in group.map_err(|err| refuse(err.to_string()))?.into_types() {
                        signatures.push(match &ty.composite_type.inner {
                            CompositeInnerType::Func(func) => (
                                func.params().iter().map(|t| letter(*t)).collect(),
                                func.results().iter().map(|t| letter(*t)).collect(),
                            ),
                            // Not a function type: never named by a function import.
                            _ => (String::new(), String::new()),
                        });
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for group in reader {
                    for entry in group.map_err(|err| refuse(err.to_string()))? {
                        let (_, import) = entry.map_err(|err| refuse(err.to_string()))?;
                        let index = match modules
                            .iter()
                            .position(|(name, ..)| name == import.module)
                        {
                            Some(index) => index,
                            None => {
                                modules.push((import.module.to_string(), Vec::new(), Vec::new()));
                                modules.len() - 1
                            }
                        };
                        let (_, functions, globals) = &mut modules[index];
                        let qualified = format!("{}.{}", import.module, import.name);

                        match import.ty {
                            TypeRef::Func(ty) | TypeRef::FuncExact(ty) => {
                                let (params, returns) =
                                    signatures.get(ty as usize).ok_or_else(|| {
                                        refuse(format!("import {qualified} names a missing type"))
                                    })?;
                                let function = HostFunction::try_new(
                                    name(&qualified, import.name)?,
                                    HostValList::try_new(params).map_err(|err| {
                                        refuse(format!("import {qualified}: {err:?}"))
                                    })?,
                                    HostValList::try_new(returns).map_err(|err| {
                                        refuse(format!("import {qualified}: {err:?}"))
                                    })?,
                                    |_, _| ControlFlow::Break(HostFunctionBreak::Trap),
                                )
                                .map_err(|err| refuse(format!("import {qualified}: {err:?}")))?;
                                functions.push(function);
                            }
                            TypeRef::Global(global) => {
                                globals.push(HostGlobal {
                                    name: name(&qualified, import.name)?,
                                    value: spacewasm::Box::new(Stub {
                                        ty: valtype(global.content_type).ok_or_else(|| {
                                            refuse(format!(
                                                "import {qualified} has an unsupported type"
                                            ))
                                        })?,
                                        mutable: global.mutable,
                                    })
                                    .map_err(ReplayError::Alloc)?
                                    .into_global_value_dyn(),
                                });
                            }
                            // Neither changes the IR, but the flight store would need
                            // them to load at all, and a stub cannot stand in for one.
                            TypeRef::Memory(_) | TypeRef::Table(_) | TypeRef::Tag(_) => {
                                return Err(refuse(format!(
                                    "import {qualified} is not a function or global, so the \
                                     load cannot be replayed without the host's own"
                                )));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let mut out = spacewasm::Vec::new(modules.len())?;
    for (module_name, functions, globals) in modules {
        out.try_push(HostModule {
            name: name(&module_name, &module_name)?,
            globals: into_vec(globals)?,
            functions: into_vec(functions)?,
            memory: spacewasm::Vec::new(0)?,
            table: spacewasm::Vec::new(0)?,
        })?;
    }
    Ok(out)
}

fn into_vec<T>(items: Vec<T>) -> Result<spacewasm::Vec<T>, ReplayError> {
    let mut out = spacewasm::Vec::new(items.len())?;
    for item in items {
        out.try_push(item)?;
    }
    Ok(out)
}

fn name<const N: usize>(
    qualified: &str,
    name: &str,
) -> Result<spacewasm::HostName<N>, ReplayError> {
    spacewasm::HostName::try_from_str(name).map_err(|_| ReplayError::Rejected {
        offset: 0,
        reason: format!("import {qualified}: the name is longer than spacewasm allows ({N})"),
    })
}

/// The `iIfd` spelling `HostValList` takes.
fn letter(ty: wasmparser::ValType) -> char {
    match ty {
        wasmparser::ValType::I32 => 'i',
        wasmparser::ValType::I64 => 'I',
        wasmparser::ValType::F32 => 'f',
        wasmparser::ValType::F64 => 'd',
        // Not in WebAssembly 1.0; `HostValList` refuses it with a clear error.
        wasmparser::ValType::V128 | wasmparser::ValType::Ref(_) => '?',
    }
}

fn valtype(ty: wasmparser::ValType) -> Option<ValType> {
    match ty {
        wasmparser::ValType::I32 => Some(ValType::I32),
        wasmparser::ValType::I64 => Some(ValType::I64),
        wasmparser::ValType::F32 => Some(ValType::F32),
        wasmparser::ValType::F64 => Some(ValType::F64),
        wasmparser::ValType::V128 | wasmparser::ValType::Ref(_) => None,
    }
}

/// A global that type-checks the import and holds nothing. Reading it during a replay
/// would mean the replay ran code, which it does not.
struct Stub {
    ty: ValType,
    mutable: bool,
}

impl GlobalValue for Stub {
    fn write(&self, _: Value) -> Result<(), GlobalValueError> {
        Err(GlobalValueError)
    }

    fn read(&self) -> Result<Value, GlobalValueError> {
        Err(GlobalValueError)
    }

    fn ty(&self) -> ValType {
        self.ty
    }

    fn mutable(&self) -> bool {
        self.mutable
    }
}
