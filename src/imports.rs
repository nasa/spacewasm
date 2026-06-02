use crate::*;

/// A general purpose
#[derive(Debug, Clone, Copy)]
pub struct ExternalRef {
    pub module: ExternalModuleRef,
    pub index: u16,
}

/// A general purpose
#[derive(Debug, Clone, Copy)]
pub struct HostRef {
    pub module: HostModuleRef,
    pub index: u16,
}

#[derive(Debug, Clone, Copy)]
pub enum Import {
    Func {
        module: ExternalModuleRef,
        index: u16,
    },
    HostFunc {
        module: HostModuleRef,
        index: u16,
    },
    Table {
        module: ExternalModuleRef,
        index: u16,
    },
    Mem {
        module: ExternalModuleRef,
        index: u16,
    },
    Global {
        module: ExternalModuleRef,
        index: u16,
    },
    HostGlobal {
        module: HostModuleRef,
        index: u16,
    },
}

impl From<Import> for Ref {
    fn from(value: Import) -> Self {
        match value {
            Import::Func { module, index }
            | Import::Table { module, index }
            | Import::Mem { module, index }
            | Import::Global { module, index } => Ref::Extern { module, index },

            Import::HostFunc { module, index } | Import::HostGlobal { module, index } => {
                Ref::Host { module, index }
            }
        }
    }
}

impl Store {
    fn link_function(
        &self,
        module_name: &str,
        name: &str,
        expected_ty: &FuncType,
    ) -> Result<Import, ValidationError> {
        for (mi, module) in self.host_modules.iter().enumerate() {
            if module.name == module_name {
                for (fi, f) in module.functions.iter().enumerate() {
                    if f.name() == name {
                        // Validate the function signature
                        return if f.params() == expected_ty.params[..]
                            && f.returns() == expected_ty.returns[..]
                        {
                            Ok(Import::HostFunc {
                                module: HostModuleRef::new(mi),
                                index: fi as u16,
                            })
                        } else {
                            Err(ValidationError::FunctionImportTypeMismatch)
                        };
                    }
                }
            }
        }

        for (mi, module) in self.modules.iter().enumerate() {
            // Check if this module is the module we are looking for
            if module.name == module_name {
                // Look for any exports that match this function name
                for e in &module.exports {
                    if e.name == name {
                        // Make sure this is a function
                        return if let ExportDesc::Func(fi) = e.desc {
                            // Resolve the index to an import or local function
                            let f_ref = module
                                .imports
                                .iter()
                                .filter_map(|i| match &i {
                                    Import::Func { module, index } => Some(Ref::Extern {
                                        module: *module,
                                        index: *index,
                                    }),
                                    Import::HostFunc { module, index } => Some(Ref::Host {
                                        module: *module,
                                        index: *index,
                                    }),
                                    _ => None,
                                })
                                .skip(fi.0 as usize)
                                .next()
                                .unwrap_or(Ref::Module(
                                    (fi.0 as usize - module.func_import_count()) as u16,
                                ));

                            // Check the function signature
                            match f_ref {
                                Ref::Module(idx) => {
                                    // This function is in the current module
                                    let f = module
                                        .functions
                                        .get(idx as usize)
                                        .ok_or(ValidationError::FunctionIdxOutOfRange)?;

                                    let ty = module
                                        .types
                                        .get(f.ty.0 as usize)
                                        .ok_or(ValidationError::TypeIdxOutOfRange)?;

                                    // Check the function signature
                                    if ty == expected_ty {
                                        Ok(Import::Func {
                                            module: ExternalModuleRef(mi as u8),
                                            index: ((fi.0 as usize) - module.func_import_count())
                                                as u16,
                                        })
                                    } else {
                                        Err(ValidationError::FunctionImportTypeMismatch)
                                    }
                                }
                                Ref::Extern { module, index } => {
                                    let em = &self.modules[module.0 as usize];
                                    let f = &em.functions[index as usize];
                                    let ty = &em.types[f.ty.0 as usize];

                                    if ty != expected_ty {
                                        return Err(ValidationError::FunctionImportTypeMismatch);
                                    }

                                    Ok(Import::Func { module, index })
                                }
                                Ref::Host { module, index } => {
                                    let hm = &self.host_modules[module.0 as usize];
                                    let f = &hm.functions[index as usize];

                                    if f.params() == expected_ty.params[..]
                                        && f.returns() == expected_ty.returns[..]
                                    {
                                        Ok(Import::HostFunc { module, index })
                                    } else {
                                        Err(ValidationError::FunctionImportTypeMismatch)
                                    }
                                }
                            }
                        } else {
                            Err(ValidationError::FunctionImportTypeMismatch)
                        };
                    }
                }
            }
        }

        Err(ValidationError::FunctionImportNotFound)
    }

    fn link_global(
        &self,
        module_name: &str,
        name: &str,
        expected_ty: GlobalType,
    ) -> Result<Import, ValidationError> {
        for (mi, module) in self.host_modules.iter().enumerate() {
            if module.name == module_name {
                for (gi, g) in module.globals.iter().enumerate() {
                    if g.name == name {
                        // Validate the imported type matches the global type defined here
                        if g.value.ty() != expected_ty.ty {
                            return Err(ValidationError::GlobalImportTypeMismatch);
                        } else if !g.value.mutable() && expected_ty.mutable {
                            return Err(ValidationError::GlobalIsNotMutable);
                        }

                        return Ok(Import::HostGlobal {
                            module: HostModuleRef::new(mi),
                            index: gi as u16,
                        });
                    }
                }
            }
        }

        for (mi, module) in self.modules.iter().enumerate() {
            if module.name == module_name {
                for e in &module.exports {
                    if e.name == name {
                        return if let ExportDesc::Global(gi) = &e.desc {
                            // This index could either be an import or a global
                            // in the current module
                            let g_ref = module
                                .imports
                                .iter()
                                .filter_map(|i| match &i {
                                    Import::Global { module, index } => Some(Ref::Extern {
                                        module: *module,
                                        index: *index,
                                    }),
                                    Import::HostGlobal { module, index } => Some(Ref::Host {
                                        module: *module,
                                        index: *index,
                                    }),
                                    _ => None,
                                })
                                .skip(gi.0 as usize)
                                .next()
                                .unwrap_or(Ref::Module(
                                    (gi.0 as usize - module.global_import_count()) as u16,
                                ));

                            // Check the function signature
                            match g_ref {
                                Ref::Module(idx) => {
                                    let g = &module.globals[idx as usize];
                                    if g.type_.ty != expected_ty.ty {
                                        Err(ValidationError::GlobalImportTypeMismatch)
                                    } else if expected_ty.mutable && !g.type_.mutable {
                                        Err(ValidationError::GlobalIsNotMutable)
                                    } else {
                                        Ok(Import::Global {
                                            module: ExternalModuleRef(mi as u8),
                                            index: idx,
                                        })
                                    }
                                }
                                Ref::Extern { module, index } => {
                                    let em = &self.modules[module.0 as usize];
                                    let g = &em.globals[index as usize];
                                    if g.type_.ty != expected_ty.ty {
                                        Err(ValidationError::GlobalImportTypeMismatch)
                                    } else if expected_ty.mutable && !g.type_.mutable {
                                        Err(ValidationError::GlobalIsNotMutable)
                                    } else {
                                        Ok(Import::Global { module, index })
                                    }
                                }
                                Ref::Host { module, index } => {
                                    let hm = &self.host_modules[module.0 as usize];
                                    let g = &hm.globals[index as usize];
                                    if g.value.ty() != expected_ty.ty {
                                        Err(ValidationError::GlobalImportTypeMismatch)
                                    } else if expected_ty.mutable && !g.value.mutable() {
                                        Err(ValidationError::GlobalIsNotMutable)
                                    } else {
                                        Ok(Import::HostGlobal { module, index })
                                    }
                                }
                            }
                        } else {
                            Err(ValidationError::GlobalImportTypeMismatch)
                        };
                    }
                }
            }
        }

        Err(ValidationError::GlobalImportNotFound)
    }
}

impl Import {
    pub fn read(
        wasm: &mut Reader,
        module: &Module,
        store: &Store,
    ) -> Result<Import, ValidationError> {
        let module_raw = wasm.read_vec_stack::<32, _>(|r| r.read_u8())?;
        let module_name = (&module_raw).try_into()?;

        let name_raw = wasm.read_vec_stack::<32, _>(|r| r.read_u8())?;
        let name = (&name_raw).try_into()?;

        let desc = ImportDesc::read(wasm)?;

        // Look up this import given its name/module
        match desc {
            ImportDesc::Func(f) => {
                // Look up the function type from the WASM module
                let ty = module
                    .types
                    .get(f.0 as usize)
                    .ok_or(ValidationError::FunctionImportOutOfRange)?;

                store.link_function(module_name, name, ty)
            }
            ImportDesc::Table(_) => Err(ValidationError::TableImportsNotSupportedYet),
            ImportDesc::Mem(_) => Err(ValidationError::MemoryImportsNotSupportedYet),
            ImportDesc::Global(g_ty) => store.link_global(module_name, name, g_ty),
        }
    }
}
