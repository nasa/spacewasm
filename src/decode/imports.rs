use crate::*;
use core::ops::Deref;

/// A general purpose
#[derive(Debug, Clone, Copy)]
pub struct Ref {
    pub module: ModuleRef,
    pub index: u16,
}

#[derive(Debug, Clone, Copy)]
pub enum Import {
    Func { module: ModuleRef, index: u16 },
    FuncHost { module: ModuleRef, index: u16 },
    Table { module: ModuleRef, index: u16 },
    Mem { module: ModuleRef, index: u16 },
    Global { module: ModuleRef, index: u16 },
    GlobalHost { module: ModuleRef, index: u16 },
}

impl From<Import> for Ref {
    fn from(value: Import) -> Self {
        match value {
            Import::Func { module, index }
            | Import::FuncHost { module, index }
            | Import::Table { module, index }
            | Import::Mem { module, index }
            | Import::Global { module, index }
            | Import::GlobalHost { module, index } => Ref { module, index },
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
        for (mi, module) in self.0.iter().enumerate() {
            match module.deref() {
                StoreModule::Host(hm) => {
                    if hm.name == module_name {
                        for (fi, f) in hm.functions.iter().enumerate() {
                            if f.name() == name {
                                // Validate the function signature
                                return if f.params() == expected_ty.params[..]
                                    && f.returns() == expected_ty.returns[..]
                                {
                                    Ok(Import::FuncHost {
                                        module: ModuleRef::new(self, mi),
                                        index: fi as u16,
                                    })
                                } else {
                                    Err(ValidationError::FunctionImportTypeMismatch)
                                };
                            }
                        }
                    }
                }
                StoreModule::Module(wm) => {
                    // Check if this module is the module we are looking for
                    if wm.name == module_name {
                        // Look for any exports that match this function name
                        for e in &wm.exports {
                            if e.name == name {
                                // Make sure this is a function
                                return if let ExportDesc::Func(fi) = e.desc {
                                    // This index could either be an import or a function
                                    // in the current module
                                    if (fi.0 as usize) < wm.func_import_count() {
                                        // This function index is an import
                                        // The function reference is relative to _this_ module
                                        // We will need to convert the module reference index to be
                                        // relative to the current module under construction
                                        let wm_relative_ref = wm
                                            .imports
                                            .iter()
                                            .filter_map(|i| match &i {
                                                Import::Global { module, index } => Some(Ref {
                                                    module: *module,
                                                    index: *index,
                                                }),
                                                _ => None,
                                            })
                                            .skip(fi.0 as usize)
                                            .next()
                                            .unwrap();

                                        // Check the function signature

                                        Ok(Import::Func {
                                            module: ModuleRef::new(self, mi),
                                            index: wm_relative_ref.index,
                                        })
                                    } else {
                                        // This function is in the current module
                                        let f = wm
                                            .functions
                                            .get((fi.0 as usize) - wm.func_import_count())
                                            .ok_or(ValidationError::FunctionIdxOutOfRange)?;

                                        let ty = wm
                                            .types
                                            .get(f.ty.0 as usize)
                                            .ok_or(ValidationError::TypeIdxOutOfRange)?;

                                        // Check the function signature
                                        if ty == expected_ty {
                                            Ok(Import::Func {
                                                module: ModuleRef::new(self, mi),
                                                index: ((fi.0 as usize) - wm.func_import_count())
                                                    as u16,
                                            })
                                        } else {
                                            Err(ValidationError::FunctionImportTypeMismatch)
                                        }
                                    }
                                } else {
                                    Err(ValidationError::FunctionImportTypeMismatch)
                                };
                            }
                        }
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
        for (mi, module) in self.0.iter().enumerate() {
            match module.deref() {
                StoreModule::Host(hm) => {
                    if hm.name == module_name {
                        for (gi, g) in hm.globals.iter().enumerate() {
                            if g.name == name {
                                // Validate the imported type matches the global type defined here
                                if g.value.ty() != expected_ty.ty {
                                    return Err(ValidationError::GlobalImportTypeMismatch);
                                } else if !g.value.mutable() && expected_ty.mutable {
                                    return Err(ValidationError::GlobalIsNotMutable);
                                }

                                return Ok(Import::GlobalHost {
                                    module: ModuleRef::new(self, mi),
                                    index: gi as u16,
                                });
                            }
                        }
                    }
                }
                StoreModule::Module(wm) => if wm.name == module_name {
                    
                },
            }
        }

        Err(ValidationError::GlobalImportNotFound)
    }
}

impl Import {
    fn import_function() {}

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
