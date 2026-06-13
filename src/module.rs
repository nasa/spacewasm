use crate::*;

#[derive(Clone)]
pub enum MemoryKind {
    Owned(Rc<Memory>),
    Import(ModuleRef),
    ImportHost(HostModuleRef),
}

#[derive(Clone)]
pub struct Module {
    pub name: String,
    pub types: Vec<FuncType>,
    pub functions: Vec<Func>,
    pub table: Vec<Ref>,
    pub memory: Option<MemoryKind>,
    pub globals: Vec<Global>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub start: Option<Ref>,
    pub table_defined: bool, // FIXME(tumbar) This feels sort of pointless?
}

pub trait CustomSectionHandler {
    /// Called when we reach a custom section of the WASM binary.
    /// This reader must read _exactly_ [size] bytes out from the reader.
    /// If the reader does not follow this rule, a validation error will be triggered.
    fn custom_section(
        &mut self,
        name: &str,
        size: usize,
        reader: &mut Reader,
    ) -> Result<(), ValidationError>;
}

struct DefaultCustomSectionHandler;
impl CustomSectionHandler for DefaultCustomSectionHandler {
    fn custom_section(
        &mut self,
        name: &str,
        size: usize,
        reader: &mut Reader,
    ) -> Result<(), ValidationError> {
        let _ = name;
        reader.skip(size)
    }
}

impl Module {
    pub fn new<const N: usize>(
        name: &str,
        stream: &mut dyn WasmStream,
        store: &StoreLinker,
        code_builder: &mut CodeBuilder<N>,
        allocator: &'static dyn WasmMemoryAllocator,
        compiler_options: CompilerOptions,
    ) -> Result<Module, ParseError> {
        let mut wasm = Reader::new(stream);

        Module::read::<N>(
            name,
            &mut wasm,
            store,
            code_builder,
            &mut DefaultCustomSectionHandler,
            allocator,
            None,
            compiler_options,
        )
        .map_err(|err| ParseError {
            offset: wasm.offset() as u32,
            err: err.into(),
        })
    }

    pub fn new_with_statistics<const N: usize>(
        name: &str,
        stream: &mut dyn WasmStream,
        store: &StoreLinker,
        code_builder: &mut CodeBuilder<N>,
        allocator: &'static dyn WasmMemoryAllocator,
        compiler_options: CompilerOptions,
    ) -> Result<(Module, [MemoryStatistics; SectionKind::N as usize]), ParseError> {
        let mut wasm = Reader::new(stream);
        let mut stats: [MemoryStatistics; SectionKind::N as usize] = Default::default();

        let m = Module::read::<N>(
            name,
            &mut wasm,
            store,
            code_builder,
            &mut DefaultCustomSectionHandler,
            allocator,
            Some(&mut stats),
            compiler_options,
        )
        .map_err(|err| ParseError {
            offset: wasm.offset() as u32,
            err: err.into(),
        })?;

        Ok((m, stats))
    }

    fn read<const N: usize>(
        name: &str,
        wasm: &mut Reader,
        store: &StoreLinker,
        code_builder: &mut CodeBuilder<N>,
        custom_handler: &mut dyn CustomSectionHandler,
        allocator: &'static dyn WasmMemoryAllocator,
        mut stats: Option<&mut [MemoryStatistics; SectionKind::N as usize]>,
        compiler_options: CompilerOptions,
    ) -> Result<Module, SectionDecodeError> {
        let magic = wasm.strip_bytes::<4>()?;
        if magic != [0x00, 0x61, 0x73, 0x6D] {
            return Err(ValidationError::MalformedMagic.into());
        }

        let version = wasm.strip_bytes::<4>()?;

        if version != [0x01, 0x00, 0x00, 0x00] {
            return Err(ValidationError::MalformedVersion.into());
        }

        // Make sure that the module name is not a duplicate in the store
        if let Some(_) = store.modules.iter().find(|m| m.name == name) {
            return Err(ValidationError::DuplicateModuleName.into());
        }

        if let Some(_) = store.host_modules.iter().find(|m| m.name == name) {
            return Err(ValidationError::DuplicateModuleName.into());
        }

        let mut module = Module {
            name: name.try_into()?,
            types: Vec::zero(),
            functions: Vec::zero(),
            table: Vec::zero(),
            memory: None,
            globals: Vec::zero(),
            imports: Vec::zero(),
            exports: Vec::zero(),
            start: None,
            table_defined: false,
        };

        let mut last_section: SectionKind = SectionKind::Custom;
        let mut seen_code_section = false;

        loop {
            let section_ty = match SectionKind::read(wasm) {
                Ok(section) => section,
                Err(ValidationError::Eof) => break,
                Err(e) => return Err(e.into()),
            };

            // Validate the section ordering
            // Custom sections can be interspersed as needed
            if section_ty != SectionKind::Custom {
                if last_section > section_ty && last_section != SectionKind::Custom {
                    return Err(
                        ValidationError::InvalidSectionOrdering(last_section, section_ty).into(),
                    );
                } else if last_section == section_ty {
                    return Err(ValidationError::DuplicateSection(section_ty).into());
                }

                last_section = section_ty;
            }

            if section_ty == SectionKind::Code {
                seen_code_section = true;
            }

            let section_size = wasm.read_u32()? as usize;
            let section_start = wasm.offset();

            let memory_before = GlobalAllocator.memory_statistics();

            module
                .read_section(
                    wasm,
                    store,
                    section_size,
                    section_ty,
                    custom_handler,
                    code_builder,
                    allocator,
                    compiler_options,
                )
                .map_err(|e| e.with_section(section_ty))?;

            let memory_after = GlobalAllocator.memory_statistics();

            // Compute the memory usage delta to track per-section usage
            if let Some(stats) = &mut stats {
                stats[section_ty as usize] += memory_after - memory_before;
            }

            // Validate we actually read the entire section
            let section_end = wasm.offset();
            let section_length = section_end - section_start;
            if section_length != section_size {
                return Err(ValidationError::MalformedSectionSize.with_section(section_ty));
            }
        }

        // If we have a function section we should also have a code section and vis versa
        if !seen_code_section && module.functions.len() > 0 {
            return Err(ValidationError::InvalidCodeSectionFunctionCount.into());
        }

        Ok(module)
    }

    fn read_section<const PN: usize>(
        &mut self,
        wasm: &mut Reader,
        store: &StoreLinker,
        section_size: usize,
        section_ty: SectionKind,
        custom_handler: &mut dyn CustomSectionHandler,
        code_builder: &mut CodeBuilder<PN>,
        allocator: &'static dyn WasmMemoryAllocator,
        compiler_options: CompilerOptions,
    ) -> Result<(), ValidationError> {
        use SectionKind::*;
        match section_ty {
            Custom => {
                CustomSection::read(wasm, section_size, custom_handler)?;
            }
            Type => {
                self.types = TypeSection::read(wasm)?;
            }
            Import => {
                self.imports = ImportSection::read(wasm, store, self)?;

                // We need to resolve some imports into the module (i.e. memory and table)
                for import in &self.imports {
                    match import {
                        crate::Import::Mem { module } => {
                            if self.memory.is_some() {
                                return Err(ValidationError::MultipleMemories);
                            }

                            // The import should have already validated with linkage
                            // We can make the assertion here
                            let Some(MemoryKind::Owned(_)) =
                                &store.modules[module.0 as usize].memory
                            else {
                                unreachable!()
                            };

                            self.memory = Some(MemoryKind::Import(*module));
                        }
                        crate::Import::HostMem { module } => {
                            if self.memory.is_some() {
                                return Err(ValidationError::MultipleMemories);
                            }

                            // Make sure this module actually defines a memory
                            let Some(_) = &store.host_modules[module.0 as usize].memory else {
                                unreachable!()
                            };

                            self.memory = Some(MemoryKind::ImportHost(*module))
                        }
                        _ => {}
                    }
                }
            }
            Function => {
                self.functions = FunctionSection::read(wasm, self)?;
            }
            Table => {
                self.table = TableSection::read(wasm)?;
                self.table_defined = true;
            }
            Memory => {
                self.memory = MemorySection::read(wasm, self, allocator)?;
            }
            Global => {
                self.globals = GlobalSection::read(wasm, store, self)?;
            }
            Export => {
                self.exports = ExportSection::read(wasm, self)?;
            }
            Start => {
                let idx = FuncIdx::read(wasm)?;
                let r = self
                    .get_func_ref(idx)
                    .ok_or(ValidationError::FunctionIdxOutOfRange)?;

                // Start functions must be [] -> []
                match r {
                    Ref::Module(index) => {
                        let f = &self.functions[index as usize];
                        let ty = &self.types[f.ty.0 as usize];
                        if ty.params.len() != 0 || ty.returns.len() != 0 {
                            return Err(ValidationError::InvalidStartFunctionSignature);
                        }
                    }
                    Ref::Host { module, index } => {
                        let f = &store.host_modules[module.0 as usize].functions[index as usize];
                        if f.params().len() != 0 || f.returns().len() != 0 {
                            return Err(ValidationError::InvalidStartFunctionSignature);
                        }
                    }
                    Ref::Extern { module, index } => {
                        let m = &store.modules[module.0 as usize];
                        let f = &m.functions[index as usize];
                        let ty = &m.types[f.ty.0 as usize];
                        if ty.params.len() != 0 || ty.returns.len() != 0 {
                            return Err(ValidationError::InvalidStartFunctionSignature);
                        }
                    }
                }

                self.start = Some(r);
            }
            Element => {
                ElementSection::read(wasm, store, self)?;
            }
            Code => {
                CodeSection::read::<PN>(wasm, code_builder, store, self, compiler_options)?;
            }
            Data => {
                DataSection::read(wasm, store, self)?;
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn get_func_ref(&self, x: FuncIdx) -> Option<Ref> {
        // Check if this function index is a host function or an internal function
        let mut n = 0;
        for f in self.imports.iter() {
            match f {
                Import::Func { module, index } => {
                    if x.0 == n {
                        // We are at the proper index, this is our function
                        // This import has already been resolved to an embedded function
                        return Some(Ref::Extern {
                            module: *module,
                            index: *index,
                        });
                    }

                    n += 1;
                }
                Import::HostFunc { module, index } => {
                    if x.0 == n {
                        return Some(Ref::Host {
                            module: *module,
                            index: *index,
                        });
                    }

                    n += 1;
                }
                _ => {}
            }
        }

        // 'n' is the number of imported functions which offset the local function index
        let i = (x.0 - n) as usize;

        if i >= self.functions.len() {
            None
        } else {
            Some(Ref::Module(i as u16))
        }
    }

    pub fn get_global_ref(&self, x: GlobalIdx) -> Option<Ref> {
        // Check if this global index is a host function or an internal global
        let mut n = 0;
        for f in self.imports.iter() {
            match f {
                Import::Global { module, index } => {
                    if x.0 == n {
                        // We are at the proper index, this is our global
                        // This import has already been resolved to an embedded global
                        return Some(Ref::Extern {
                            module: *module,
                            index: *index,
                        });
                    }

                    n += 1;
                }
                Import::HostGlobal { module, index } => {
                    if x.0 == n {
                        return Some(Ref::Host {
                            module: *module,
                            index: *index,
                        });
                    }

                    n += 1;
                }
                _ => {}
            }
        }

        // 'n' is the number of imported globals which offset the internal global index
        let i = (x.0 - n) as usize;

        if i >= self.globals.len() {
            None
        } else {
            Some(Ref::Module(i as u16))
        }
    }
}

/// All WASM sections ordered by the order expected in the file
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionKind {
    Custom,
    Type,
    Import,
    Function,
    Table,
    Memory,
    Global,
    Export,
    Start,
    Element,
    Code,
    Data,

    N,
}

impl SectionKind {
    pub fn convert(value: u8) -> Result<SectionKind, ValidationError> {
        use SectionKind::*;
        let ty = match value {
            0 => Custom,
            1 => Type,
            2 => Import,
            3 => Function,
            4 => Table,
            5 => Memory,
            6 => Global,
            7 => Export,
            8 => Start,
            9 => Element,
            10 => Code,
            11 => Data,
            other => return Err(ValidationError::MalformedSectionId(other)),
        };

        Ok(ty)
    }

    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        SectionKind::convert(wasm.read_u8()?)
    }
}

pub struct CustomSection;

impl CustomSection {
    pub fn read(
        wasm: &mut Reader,
        size: usize,
        handler: &mut dyn CustomSectionHandler,
    ) -> Result<(), ValidationError> {
        let start = wasm.offset();
        let name: StaticVec<u8, 32> = wasm.read_vec_stack(|w| w.read_u8())?;
        let name_str = core::str::from_utf8(&name).map_err(|_| ValidationError::MalformedUtf8)?;

        let name_length = wasm.offset() - start;
        if name_length > size {
            return Err(ValidationError::MalformedSectionSize);
        }

        handler.custom_section(name_str, size - name_length, wasm)
    }
}

pub struct TypeSection;

impl TypeSection {
    pub fn read(wasm: &mut Reader) -> Result<Vec<FuncType>, ValidationError> {
        wasm.read_vec(FuncType::read)
    }
}

macro_rules! read_impl_u32 {
    ($type_name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $type_name(pub u32);
        impl $type_name {
            pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
                Ok($type_name(wasm.read_u32()?))
            }
        }

        impl From<u32> for $type_name {
            fn from(v: u32) -> Self {
                $type_name(v)
            }
        }
    };
}

read_impl_u32!(TypeIdx);
read_impl_u32!(FuncIdx);
read_impl_u32!(TableIdx);
read_impl_u32!(MemIdx);
read_impl_u32!(GlobalIdx);
read_impl_u32!(LocalIdx);
read_impl_u32!(LabelIdx);

pub enum ImportDesc {
    Func(TypeIdx),
    Table(TableType),
    Mem(MemType),
    Global(GlobalType),
}

impl ImportDesc {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        match wasm.read_u8()? {
            0x00 => Ok(ImportDesc::Func(TypeIdx::read(wasm)?)),
            0x01 => Ok(ImportDesc::Table(TableType::read(wasm)?)),
            0x02 => Ok(ImportDesc::Mem(MemType::read(wasm)?)),
            0x03 => Ok(ImportDesc::Global(GlobalType::read(wasm)?)),
            c => Err(ValidationError::MalformedImportExportDesc(c)),
        }
    }
}

pub struct ImportSection;

impl ImportSection {
    pub fn read(
        wasm: &mut Reader,
        store: &StoreLinker,
        module: &Module,
    ) -> Result<Vec<Import>, ValidationError> {
        wasm.read_vec(|w| Import::read(w, module, store))
    }
}

impl Func {
    pub fn read_func_section(wasm: &mut Reader, module: &Module) -> Result<Func, ValidationError> {
        let ty_idx = TypeIdx::read(wasm)?;

        let ty = module
            .types
            .get(ty_idx.0 as usize)
            .ok_or(ValidationError::TypeIdxOutOfRange)?;

        let parameter_size = ty.params.iter().fold(0, |sum, a_ty| sum + a_ty.size()) / 4;

        let return_ty = if ty.returns.len() > 1 {
            return Err(ValidationError::FunctionReturnsTooLarge);
        } else if ty.returns.len() == 0 {
            None
        } else {
            Some(ty.returns[0].clone())
        };

        if parameter_size > 0xFF {
            return Err(ValidationError::FunctionParametersTooLarge);
        }

        Ok(Func {
            ty: ty_idx,
            stack_usage: 0,
            local_size: 0,
            parameter_size: parameter_size as u8,
            return_ty,
            locals: Vec::zero(),
            expr: Expr::zero(),
        })
    }
}

struct FunctionSection;
impl FunctionSection {
    pub fn read(wasm: &mut Reader, module: &Module) -> Result<Vec<Func>, ValidationError> {
        wasm.read_vec(|w| Func::read_func_section(w, module))
    }
}

pub struct TableSection;

impl TableSection {
    pub fn read(wasm: &mut Reader) -> Result<Vec<Ref>, ValidationError> {
        let n = wasm.read_u32()?;
        if n == 0 {
            Ok(Vec::zero())
        } else if n == 1 {
            let table_type = TableType::read(wasm)?;
            let mut v = Vec::new(table_type.limits.min)?;
            for _ in 0..table_type.limits.min {
                v.push(Ref::Module(0xFFFF))
            }

            Ok(v)
        } else {
            Err(ValidationError::InvalidTableIndex)
        }
    }
}

pub struct MemorySection;
impl MemorySection {
    pub fn read(
        wasm: &mut Reader,
        module: &Module,
        allocator: &'static dyn WasmMemoryAllocator,
    ) -> Result<Option<MemoryKind>, ValidationError> {
        let len = wasm.read_u32()?;
        if len > 1 {
            Err(ValidationError::MultipleMemories)
        } else if len == 0 {
            Ok(None)
        } else if module.memory.is_some() {
            return Err(ValidationError::MultipleMemories);
        } else {
            // We are allocating memory for this module
            let ty = MemType::read(wasm)?;
            let memory = Memory::new(ty, allocator)?;
            Ok(Some(MemoryKind::Owned(Rc::new(memory)?)))
        }
    }
}

#[derive(Clone)]
pub struct Global {
    pub type_: GlobalType,
    pub value: RawValue,
}

impl Global {
    pub fn value(&self) -> Value {
        self.value.to_value(self.type_.ty)
    }

    pub fn read(
        wasm: &mut Reader,
        store: &StoreLinker,
        module: &Module,
    ) -> Result<Self, ValidationError> {
        let type_ = GlobalType::read(wasm)?;
        let init = Expr::read_constant(wasm, store, module)?;
        let init = match init {
            Value::I32(i) => {
                if type_.ty != ValType::I32 {
                    return Err(ValidationError::GlobalTypeMismatch);
                }

                RawValue::from_i32(i)
            }
            Value::I64(i) => {
                if type_.ty != ValType::I64 {
                    return Err(ValidationError::GlobalTypeMismatch);
                }

                RawValue::from_i64(i)
            }
            Value::F32(z) => {
                if type_.ty != ValType::F32 {
                    return Err(ValidationError::GlobalTypeMismatch);
                }

                RawValue::from_f32(z)
            }
            Value::F64(z) => {
                if type_.ty != ValType::F64 {
                    return Err(ValidationError::GlobalTypeMismatch);
                }

                RawValue::from_f64(z)
            }
        };

        Ok(Global { type_, value: init })
    }
}

pub struct GlobalSection;
impl GlobalSection {
    pub fn read(
        wasm: &mut Reader,
        store: &StoreLinker,
        module: &Module,
    ) -> Result<Vec<Global>, ValidationError> {
        wasm.read_vec(|wasm| Global::read(wasm, store, module))
    }
}

#[derive(Clone)]
pub enum ExportDesc {
    Func(FuncIdx),
    Table(TableIdx),
    Mem(MemIdx),
    Global(GlobalIdx),
}

impl ExportDesc {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        match wasm.read_u8()? {
            0x00 => Ok(ExportDesc::Func(FuncIdx::read(wasm)?)),
            0x01 => Ok(ExportDesc::Table(TableIdx::read(wasm)?)),
            0x02 => Ok(ExportDesc::Mem(MemIdx::read(wasm)?)),
            0x03 => Ok(ExportDesc::Global(GlobalIdx::read(wasm)?)),
            c => Err(ValidationError::MalformedImportExportDesc(c)),
        }
    }
}

#[derive(Clone)]
pub struct Export {
    pub name: String,
    pub desc: ExportDesc,
}

impl Export {
    pub fn read(wasm: &mut Reader, module: &Module) -> Result<Self, ValidationError> {
        let name = Name::read(wasm)?;
        let desc = ExportDesc::read(wasm)?;

        // Check if the export refers to a defined symbol
        match desc {
            ExportDesc::Func(i) => {
                let _ = module
                    .get_func_ref(i)
                    .ok_or(ValidationError::FunctionIdxOutOfRange)?;
            }
            ExportDesc::Table(i) => {
                if i.0 != 0 {
                    return Err(ValidationError::InvalidTableIndex);
                } else if !module.table_defined {
                    return Err(ValidationError::TableNotDefined);
                }
            }
            ExportDesc::Mem(i) => {
                if i.0 != 0 {
                    return Err(ValidationError::InvalidMemIndex);
                } else if module.memory.is_none() {
                    return Err(ValidationError::MemoryNotDefined);
                }
            }
            ExportDesc::Global(i) => {
                let _ = module
                    .get_global_ref(i)
                    .ok_or(ValidationError::GlobalIdxOutOfRange)?;
            }
        }

        Ok(Export { name, desc })
    }
}

pub struct ExportSection;
impl ExportSection {
    pub fn read(wasm: &mut Reader, module: &Module) -> Result<Vec<Export>, ValidationError> {
        let len = wasm.read_u32()?;
        let mut out: Vec<Export> = Vec::new(len)?;
        for _ in 0..len {
            let e = Export::read(wasm, module)?;
            // Check for duplicate export name
            if out.iter().find(|ei| &*ei.name == &*e.name).is_some() {
                return Err(ValidationError::DuplicateExportName);
            }

            out.push(e);
        }

        Ok(out)
    }
}

pub struct Element;

impl Element {
    pub fn read(
        wasm: &mut Reader,
        store: &StoreLinker,
        module: &mut Module,
    ) -> Result<(), ValidationError> {
        let table = TableIdx::read(wasm)?;
        if table.0 != 0 {
            return Err(ValidationError::InvalidTableIndex);
        }

        if !module.table_defined {
            return Err(ValidationError::TableNotDefined);
        }

        let Value::I32(offset) = Expr::read_constant(wasm, store, module)? else {
            return Err(ValidationError::InvalidElementOffset);
        };

        let init = wasm.read_vec(FuncIdx::read)?;
        if (offset as usize + init.len()) > module.table.len() {
            return Err(ValidationError::InvalidElementOutOfBounds);
        }

        // Write the function indexes into the table
        for (i, idx) in init.iter().enumerate() {
            let r = module
                .get_func_ref(*idx)
                .ok_or(ValidationError::FunctionIdxOutOfRange)?;
            if let Ref::Extern { .. } = &r {
                return Err(ValidationError::FunctionCallsAcrossModuleNotSupportedYet);
            }

            module.table[(offset as usize) + i] = r;
        }

        Ok(())
    }
}

pub struct ElementSection;
impl ElementSection {
    pub fn read(
        wasm: &mut Reader,
        store: &StoreLinker,
        module: &mut Module,
    ) -> Result<(), ValidationError> {
        let len = wasm.read_u32()?;
        for _ in 0..len {
            Element::read(wasm, store, module)?;
        }

        Ok(())
    }
}

pub struct CodeSection;

impl CodeSection {
    pub fn read<const N: usize>(
        wasm: &mut Reader,
        builder: &mut CodeBuilder<N>,
        store: &StoreLinker,
        module: &mut Module,
        compiler_options: CompilerOptions,
    ) -> Result<(), ValidationError> {
        let n = wasm.read_u32()?;
        if n as usize != module.functions.len() {
            return Err(ValidationError::InvalidCodeSectionFunctionCount);
        }

        for i in 0..n as usize {
            module.read_function_code(wasm, store, builder, i, compiler_options)?;
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct Data {
    pub offset: u32,
    pub init: Vec<u8>,
}

impl Module {
    pub(crate) fn check_memory_defined(&self) -> Result<(), ValidationError> {
        if self.memory.is_none() {
            Err(ValidationError::MemoryNotDefined)
        } else {
            Ok(())
        }
    }
}

impl Data {
    pub fn read(
        wasm: &mut Reader,
        store: &StoreLinker,
        module: &Module,
    ) -> Result<Self, ValidationError> {
        let mem = MemIdx::read(wasm)?;
        if mem.0 != 0 {
            return Err(ValidationError::InvalidMemIndex);
        }

        // Make sure we actually defined a linear memory for this module
        module.check_memory_defined()?;

        let offset = Expr::read_constant(wasm, store, module)?;
        let init = wasm.read_vec(|w| w.read_u8())?;

        let offset = match offset {
            Value::I32(i) => {
                if i < 0 {
                    return Err(ValidationError::InvalidNegativeMemOffset);
                }

                i as u32
            }
            Value::I64(_) => return Err(ValidationError::InvalidMemOffsetType),
            Value::F32(_) => return Err(ValidationError::InvalidMemOffsetType),
            Value::F64(_) => return Err(ValidationError::InvalidMemOffsetType),
        };

        Ok(Data { offset, init })
    }
}

pub struct DataSection;
impl DataSection {
    pub fn read(
        wasm: &mut Reader,
        store: &StoreLinker,
        module: &Module,
    ) -> Result<(), ValidationError> {
        let len = wasm.read_u32()?;
        if len == 0 {
            return Ok(());
        }

        if let Some(memory) = &module.memory {
            let memory = match memory {
                MemoryKind::Owned(memory) => memory,
                MemoryKind::Import(i) => {
                    let Some(MemoryKind::Owned(memory)) = &store.modules[i.0 as usize].memory
                    else {
                        unreachable!()
                    };

                    memory
                }
                MemoryKind::ImportHost(i) => {
                    store.host_modules[i.0 as usize].memory.as_ref().unwrap()
                }
            };

            for _ in 0..len {
                let data = Data::read(wasm, store, module)?;
                memory.store(data.offset as usize, &data.init)?;
            }

            Ok(())
        } else {
            Err(ValidationError::MemoryNotDefined)
        }
    }
}
