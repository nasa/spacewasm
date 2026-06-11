use crate::*;

/// A general purpose
#[derive(Debug, Clone, Copy)]
pub struct ExternalRef {
    pub module: ModuleRef,
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
        module: ModuleRef,
        index: u16,
    },
    HostFunc {
        module: HostModuleRef,
        index: u16,
    },
    Table {
        module: ModuleRef,
        index: u16,
    },
    Mem {
        module: ModuleRef,
    },
    HostMem {
        module: HostModuleRef,
    },
    Global {
        module: ModuleRef,
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
            | Import::Global { module, index } => Ref::Extern { module, index },

            Import::HostFunc { module, index } | Import::HostGlobal { module, index } => {
                Ref::Host { module, index }
            }

            Import::Mem { module } => Ref::Extern { module, index: 0 },
            Import::HostMem { module } => Ref::Host { module, index: 0 },
        }
    }
}

impl StoreLinker {
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
                                .get_func_ref(fi)
                                .ok_or(ValidationError::FunctionIdxOutOfRange)?;

                            // Check the function signature
                            match f_ref {
                                Ref::Module(idx) => {
                                    // This function is in the current module
                                    let f = &module.functions[idx as usize];

                                    let ty = module
                                        .types
                                        .get(f.ty.0 as usize)
                                        .ok_or(ValidationError::TypeIdxOutOfRange)?;

                                    // Check the function signature
                                    if ty == expected_ty {
                                        Ok(Import::Func {
                                            module: ModuleRef(mi as u8),
                                            index: idx,
                                        })
                                    } else {
                                        Err(ValidationError::FunctionImportTypeMismatch)
                                    }
                                }
                                Ref::Extern { module, index } => {
                                    let em = &self.modules.get(module.0 as usize).unwrap();
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
                        return if g.value.ty() != expected_ty.ty {
                            Err(ValidationError::GlobalImportTypeMismatch)
                        } else if !g.value.mutable() && expected_ty.mutable {
                            Err(ValidationError::GlobalIsNotMutable)
                        } else {
                            Ok(Import::HostGlobal {
                                module: HostModuleRef::new(mi),
                                index: gi as u16,
                            })
                        };
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
                                .get_global_ref(*gi)
                                .ok_or(ValidationError::GlobalIdxOutOfRange)?;

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
                                            module: ModuleRef(mi as u8),
                                            index: idx,
                                        })
                                    }
                                }
                                Ref::Extern { module, index } => {
                                    let em = self.modules.get(module.0 as usize).unwrap();
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

    fn link_memory(
        &self,
        module_name: &str,
        name: &str,
        expected_ty: MemType,
    ) -> Result<Import, ValidationError> {
        for (mi, module) in self.host_modules.iter().enumerate() {
            if module.name == module_name {
                return if let Some(mem) = &module.memory {
                    if expected_ty.fits_in(*mem) {
                        Ok(Import::HostMem {
                            module: HostModuleRef::new(mi),
                        })
                    } else {
                        Err(ValidationError::MemoryImportTooLarge)
                    }
                } else {
                    Err(ValidationError::MemoryNotDefined)
                };
            }
        }

        for (mi, module) in self.modules.iter().enumerate() {
            // Check if this module is the module we are looking for
            if module.name == module_name {
                // Look for any exports that match this function name
                for e in &module.exports {
                    if e.name == name {
                        // Make sure this is a memory
                        return if let ExportDesc::Mem(mem_i) = e.desc {
                            // WASM 1.0 MVP only supports a single memory
                            if mem_i.0 > 0 {
                                return Err(ValidationError::MemoryIdxTooLarge);
                            }

                            match module.memory {
                                None => Err(ValidationError::MemoryNotDefined),
                                Some(MemoryKind::Allocate { index: _, ty }) => {
                                    if !expected_ty.fits_in(ty) {
                                        return Err(ValidationError::MemoryImportTooLarge);
                                    }

                                    Ok(Import::Mem {
                                        module: ModuleRef(mi as u8),
                                    })
                                }
                                Some(MemoryKind::Import(idx)) => {
                                    // This module imported memory from another module
                                    // This index is the _memory_ index not the module index
                                    // so we have to do a linear lookup.
                                    let (original_allocate_ty, import) = match self
                                        .get_memory(idx)
                                        .unwrap()
                                    {
                                        Ref::Module(_) => unreachable!(),
                                        Ref::Host { module, .. } => (
                                            self.host_modules[module.0 as usize].memory.unwrap(),
                                            Import::HostMem { module },
                                        ),
                                        Ref::Extern { module, .. } => {
                                            let MemoryKind::Allocate { ty, .. } = self.modules
                                                [module.0 as usize]
                                                .memory
                                                .as_ref()
                                                .unwrap()
                                            else {
                                                unreachable!()
                                            };

                                            (*ty, Import::Mem { module })
                                        }
                                    };

                                    if !expected_ty.fits_in(original_allocate_ty) {
                                        return Err(ValidationError::MemoryImportTooLarge);
                                    }

                                    Ok(import)
                                }
                            }
                        } else {
                            Err(ValidationError::MemoryImportTypeMismatch)
                        };
                    }
                }
            }
        }

        Err(ValidationError::MemoryImportNotFound)
    }
}

impl Import {
    pub fn read(
        wasm: &mut Reader,
        module: &Module,
        store: &StoreLinker,
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
            ImportDesc::Mem(ty) => {
                // Look up the function type from the WASM module
                store.link_memory(module_name, name, ty)
            }
            ImportDesc::Table(_) => Err(ValidationError::TableImportsNotSupportedYet),
            ImportDesc::Global(g_ty) => store.link_global(module_name, name, g_ty),
        }
    }
}
