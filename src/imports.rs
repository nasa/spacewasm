use crate::*;

/// A general purpose reference to a symbol in a host module
#[derive(Debug, Clone, Copy)]
pub struct HostRef {
    pub module: HostModuleRef,
    pub index: u16,
}

/// A reference to a Wasm symbol in the store
#[derive(Debug, Clone, Copy)]
pub struct WasmRef {
    pub module: ModuleRef,
    pub index: u16,
}

#[derive(Debug, Clone, Copy)]
pub enum Import {
    Func { module: ModuleRef, index: u16 },
    HostFunc { module: HostModuleRef, index: u16 },
    Table { module: ModuleRef, index: u16 },
    HostTable { module: HostModuleRef, index: u16 },
    Memory { module: ModuleRef, index: u16 },
    HostMemory { module: HostModuleRef, index: u16 },
    Global { module: ModuleRef, index: u16 },
    HostGlobal { module: HostModuleRef, index: u16 },
}

impl Store {
    fn link_function(
        &self,
        module_name: &str,
        name: &str,
        expected_ty: &FuncType,
    ) -> Result<Import, ValidationError> {
        for (mi, module) in self.host_modules().iter().enumerate() {
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

        for (mi, module) in self.modules().iter().enumerate() {
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
                                    let em = &self.modules().get(module.0 as usize).unwrap();
                                    let f = &em.functions[index as usize];
                                    let ty = &em.types[f.ty.0 as usize];

                                    if ty != expected_ty {
                                        return Err(ValidationError::FunctionImportTypeMismatch);
                                    }

                                    Ok(Import::Func { module, index })
                                }
                                Ref::Host { module, index } => {
                                    let hm = &self.host_modules()[module.0 as usize];
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
        for (mi, module) in self.host_modules().iter().enumerate() {
            if module.name == module_name {
                for (gi, g) in module.globals.iter().enumerate() {
                    if g.name == name {
                        // Validate the imported type matches the global type defined here
                        return if g.value.ty() != expected_ty.ty {
                            Err(ValidationError::GlobalImportTypeMismatch)
                        } else if g.value.mutable() != expected_ty.mutable {
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

        for (mi, module) in self.modules().iter().enumerate() {
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
                                    } else if expected_ty.mutable != g.type_.mutable {
                                        Err(ValidationError::GlobalIsNotMutable)
                                    } else {
                                        Ok(Import::Global {
                                            module: ModuleRef(mi as u8),
                                            index: idx,
                                        })
                                    }
                                }
                                Ref::Extern { module, index } => {
                                    let em = self.modules().get(module.0 as usize).unwrap();
                                    let g = &em.globals[index as usize];
                                    if g.type_.ty != expected_ty.ty {
                                        Err(ValidationError::GlobalImportTypeMismatch)
                                    } else if expected_ty.mutable != g.type_.mutable {
                                        Err(ValidationError::GlobalIsNotMutable)
                                    } else {
                                        Ok(Import::Global { module, index })
                                    }
                                }
                                Ref::Host { module, index } => {
                                    let hm = &self.host_modules()[module.0 as usize];
                                    let g = &hm.globals[index as usize];
                                    if g.value.ty() != expected_ty.ty {
                                        Err(ValidationError::GlobalImportTypeMismatch)
                                    } else if expected_ty.mutable != g.value.mutable() {
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

    fn link_table(
        &self,
        module_name: &str,
        name: &str,
        expected_ty: TableType,
    ) -> Result<Import, ValidationError> {
        for (mi, module) in self.host_modules().iter().enumerate() {
            if module.name == module_name {
                for (ti, table) in module.table.iter().enumerate() {
                    if table.name == name {
                        if !table.value.1.matches(&expected_ty.limits) {
                            return Err(ValidationError::TableImportIncompatibleSize);
                        }

                        return Ok(Import::HostTable {
                            module: HostModuleRef::new(mi),
                            index: ti as u16,
                        });
                    }
                }

                return Err(ValidationError::TableImportNotFound);
            }
        }

        for (mi, module) in self.modules().iter().enumerate() {
            // Check if this module is the module we are looking for
            if module.name == module_name {
                // Look for any exports that match this function name
                for e in &module.exports {
                    if e.name == name {
                        // Make sure this is a memory
                        return if let ExportDesc::Table(table_i) = e.desc {
                            // Wasm 1.0 MVP only supports a single table
                            if table_i.0 > 0 {
                                return Err(ValidationError::InvalidTableIndex);
                            }

                            let table_ty = match &module.table {
                                None => return Err(ValidationError::TableNotDefined),
                                Some(TableKind::Owned(table)) => table.1,
                                Some(TableKind::Import(import_module_ref)) => {
                                    let r = import_module_ref.0 as usize;
                                    let Some(TableKind::Owned(table)) = &self.modules()[r].table
                                    else {
                                        unreachable!()
                                    };

                                    table.1
                                }
                                Some(TableKind::ImportHost(host_import)) => {
                                    let l = self.host_modules()[host_import.module.0 as usize]
                                        .table[host_import.index as usize]
                                        .value
                                        .1;

                                    TableType {
                                        elem_type: ElemType::FuncRef,
                                        limits: l,
                                    }
                                }
                            };

                            if !table_ty.limits.matches(&expected_ty.limits) {
                                return Err(ValidationError::TableImportIncompatibleSize);
                            }

                            Ok(Import::Table {
                                module: ModuleRef(mi as u8),
                                index: 0,
                            })
                        } else {
                            Err(ValidationError::TableImportTypeMismatch)
                        };
                    }
                }
            }
        }

        Err(ValidationError::TableImportNotFound)
    }

    fn link_memory(
        &self,
        module_name: &str,
        name: &str,
        expected_ty: MemType,
    ) -> Result<Import, ValidationError> {
        for (mi, module) in self.host_modules().iter().enumerate() {
            if module.name == module_name {
                for (i, symbol) in module.memory.iter().enumerate() {
                    if name == symbol.name {
                        let mem = &symbol.value;
                        return if mem.mem_type().matches(&expected_ty) {
                            Ok(Import::HostMemory {
                                module: HostModuleRef::new(mi),
                                index: i as u16,
                            })
                        } else {
                            Err(ValidationError::MemoryImportTooLarge)
                        };
                    }
                }
            }
        }

        for (mi, module) in self.modules().iter().enumerate() {
            // Check if this module is the module we are looking for
            if module.name == module_name {
                // Look for any exports that match this function name
                for e in &module.exports {
                    if e.name == name {
                        // Make sure this is a memory
                        return if let ExportDesc::Mem(mem_i) = e.desc {
                            // Wasm 1.0 MVP only supports a single memory
                            if mem_i.0 > 0 {
                                return Err(ValidationError::InvalidMemIndex);
                            }

                            match &module.memory {
                                None => Err(ValidationError::MemoryNotDefined),
                                Some(MemoryKind::Owned(memory)) => {
                                    if !memory.mem_type().matches(&expected_ty) {
                                        return Err(ValidationError::MemoryImportTooLarge);
                                    }

                                    Ok(Import::Memory {
                                        module: ModuleRef(mi as u8),
                                        index: 0,
                                    })
                                }
                                Some(MemoryKind::Import(i)) => {
                                    // Inherit this import from the chain
                                    // This import should already be resolved to a module with
                                    // an owned memory.
                                    let Some(MemoryKind::Owned(memory)) =
                                        &self.modules()[i.0 as usize].memory
                                    else {
                                        unreachable!()
                                    };

                                    if !memory.mem_type().matches(&expected_ty) {
                                        return Err(ValidationError::MemoryImportTooLarge);
                                    }

                                    Ok(Import::Memory {
                                        module: *i,
                                        index: 0,
                                    })
                                }
                                Some(MemoryKind::ImportHost(i)) => {
                                    let symbol = &self.host_modules()[i.module.0 as usize].memory
                                        [i.index as usize];

                                    let memory = &symbol.value;
                                    if !memory.mem_type().matches(&expected_ty) {
                                        return Err(ValidationError::MemoryImportTooLarge);
                                    }

                                    Ok(Import::HostMemory {
                                        module: i.module,
                                        index: i.index,
                                    })
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
        store: &Store,
    ) -> Result<Import, ValidationError> {
        let module_raw = wasm.read_vec_stack::<32, _>(|r| r.read_u8())?;
        let module_name = (&module_raw).try_into()?;

        let name_raw = wasm.read_vec_stack::<32, _>(|r| r.read_u8())?;
        let name = (&name_raw).try_into()?;

        let desc = ImportDesc::read(wasm)?;

        // Look up this import given its name/module
        match desc {
            ImportDesc::Func(ty_idx) => {
                // Look up the function type from the Wasm module
                let ty = module
                    .types
                    .get(ty_idx.0 as usize)
                    .ok_or(ValidationError::TypeIdxOutOfRange)?;

                store.link_function(module_name, name, ty)
            }
            ImportDesc::Mem(ty) => store.link_memory(module_name, name, ty),
            ImportDesc::Table(ty) => store.link_table(module_name, name, ty),
            ImportDesc::Global(g_ty) => store.link_global(module_name, name, g_ty),
        }
    }
}
