use crate::*;

pub struct Module<'imports> {
    pub types: Vec<FuncType>,
    pub functions: Vec<Func>,
    pub tables: Vec<TableType>,
    pub memories: Vec<MemType>,
    pub globals: Vec<Global>,
    pub module_imports: ModuleImports<'imports>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub elements: Vec<Element>,
    pub data: Vec<Data>,
    pub start: Option<FuncIdx>,

    pub text: Vec<Box<TextPage>>,
    pub wasm_size: usize,
    pub final_page_offset: usize,
    pub memory_usage: [MemoryStatistics; SectionKind::N as usize],
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

impl<'imports> Module<'imports> {
    pub fn new<const N: usize>(
        stream: &mut dyn Stream,
        module_imports: ModuleImports<'imports>,
    ) -> Result<Module<'imports>, ParseError> {
        let mut wasm = Reader::new(stream);

        Module::read::<N>(&mut wasm, module_imports, &mut DefaultCustomSectionHandler).map_err(
            |err| ParseError {
                offset: wasm.offset() as u32,
                err: err.into(),
            },
        )
    }

    fn read<const N: usize>(
        wasm: &mut Reader,
        module_imports: ModuleImports<'imports>,
        custom_handler: &mut dyn CustomSectionHandler,
    ) -> Result<Module<'imports>, SectionDecodeError> {
        let magic = wasm.strip_bytes::<4>()?;
        if magic != [0x00, 0x61, 0x73, 0x6D] {
            return Err(ValidationError::MalformedMagic.into());
        }

        let version = wasm.strip_bytes::<4>()?;

        if version != [0x01, 0x00, 0x00, 0x00] {
            return Err(ValidationError::MalformedVersion.into());
        }

        let mut module = Module {
            types: Vec::zero(),
            functions: Vec::zero(),
            text: Vec::zero(),
            tables: Vec::zero(),
            memories: Vec::zero(),
            globals: Vec::zero(),
            module_imports,
            imports: Vec::zero(),
            exports: Vec::zero(),
            elements: Vec::zero(),
            data: Vec::zero(),
            start: None,
            wasm_size: 0,
            final_page_offset: 0,
            memory_usage: Default::default(),
        };

        let mut last_section: SectionKind = SectionKind::Custom;
        let mut builder = CodeBuilder::<N>::new();

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

            let section_size = wasm.read_u32()? as usize;
            let section_start = wasm.offset();

            let memory_before = GlobalAllocator.memory_statistics();

            module
                .read_section(wasm, section_size, section_ty, custom_handler, &mut builder)
                .map_err(|e| e.with_section(section_ty))?;

            let memory_after = GlobalAllocator.memory_statistics();

            // Compute the memory usage delta to track per-section usage
            module.memory_usage[section_ty as usize] += memory_after - memory_before;

            // Validate we actually read the entire section
            let section_end = wasm.offset();
            let section_length = section_end - section_start;
            if section_length != section_size {
                return Err(ValidationError::MalformedSectionSize.with_section(section_ty));
            }
        }

        let (text, text_offset) = builder.finish()?;
        module.text = text;
        module.wasm_size = wasm.offset();
        module.final_page_offset = text_offset;
        Ok(module)
    }

    fn read_section<const PN: usize>(
        &mut self,
        wasm: &mut Reader,
        section_size: usize,
        section_ty: SectionKind,
        custom_handler: &mut dyn CustomSectionHandler,
        code_builder: &mut CodeBuilder<PN>,
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
                self.imports = ImportSection::read(wasm, self)?;
            }
            Function => {
                self.functions = FunctionSection::read(wasm, self)?;
            }
            Table => {
                self.tables = TableSection::read(wasm)?;
            }
            Memory => {
                self.memories = MemorySection::read(wasm)?;
            }
            Global => {
                self.globals = GlobalSection::read(wasm)?;
            }
            Export => {
                self.exports = ExportSection::read(wasm)?;
            }
            Start => {
                self.start.replace(FuncIdx::read(wasm)?);
            }
            Element => {
                self.elements = ElementSection::read(wasm)?;
            }
            Code => {
                CodeSection::read::<PN>(wasm, code_builder, self)?;
            }
            Data => {
                self.data = DataSection::read(wasm)?;
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn get_func_ref(&self, x: FuncIdx) -> Result<FuncRef, ValidationError> {
        // Check if this function index is a host function or an internal function
        let mut n = 0;
        for f in self.imports.iter() {
            match f {
                Import::Func(fi) => {
                    if x.0 == n {
                        // We are at the proper index, this is our function
                        // This import has already been resolved to an embedded function
                        return Ok(FuncRef::HostFunc(*fi));
                    }

                    n += 1;
                }
                _ => {}
            }
        }

        // 'n' is the number of imported functions which offset the local function index
        let i = (x.0 - n) as usize;

        if i >= self.functions.len() {
            Err(ValidationError::FunctionIdxOutOfRange)
        } else {
            Ok(FuncRef::Func(i as u16))
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
        #[derive(Debug, Clone, Copy)]
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
    pub fn read(wasm: &mut Reader, module: &Module) -> Result<Vec<Import>, ValidationError> {
        wasm.read_vec(|w| Import::read(w, module))
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
        let return_size = ty.returns.iter().fold(0, |sum, a_ty| sum + a_ty.size()) / 4;

        if parameter_size > 0xFFFF {
            return Err(ValidationError::FunctionParametersTooLarge);
        }

        if return_size > 0xFF {
            return Err(ValidationError::FunctionReturnsTooLarge);
        }

        Ok(Func {
            ty: ty_idx,
            stack_usage: 0,
            local_size: 0,
            parameter_size: parameter_size as u16,
            return_size: return_size as u8,
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
    pub fn read(wasm: &mut Reader) -> Result<Vec<TableType>, ValidationError> {
        wasm.read_vec(TableType::read)
    }
}

pub struct MemorySection;
impl MemorySection {
    pub fn read(wasm: &mut Reader) -> Result<Vec<MemType>, ValidationError> {
        wasm.read_vec(MemType::read)
    }
}

pub struct Global {
    pub type_: GlobalType,
    pub init: Value,
}

impl Global {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        let type_ = GlobalType::read(wasm)?;
        let init = Expr::read_constant(wasm)?;
        Ok(Global { type_, init })
    }
}

pub struct GlobalSection;
impl GlobalSection {
    pub fn read(wasm: &mut Reader) -> Result<Vec<Global>, ValidationError> {
        wasm.read_vec(|r| Global::read(r))
    }
}

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

pub struct Export {
    pub name: String,
    pub desc: ExportDesc,
}

impl Export {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        let name = Name::read(wasm)?;
        let desc = ExportDesc::read(wasm)?;
        Ok(Export { name, desc })
    }
}

pub struct ExportSection;
impl ExportSection {
    pub fn read(wasm: &mut Reader) -> Result<Vec<Export>, ValidationError> {
        wasm.read_vec(Export::read)
    }
}

pub struct Element {
    pub table: TableIdx,
    pub offset: Value,
    pub init: Vec<FuncIdx>,
}

impl Element {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        let table = TableIdx::read(wasm)?;
        let offset = Expr::read_constant(wasm)?;
        let init = wasm.read_vec(FuncIdx::read)?;

        Ok(Element {
            table,
            offset,
            init,
        })
    }
}

pub struct ElementSection;
impl ElementSection {
    pub fn read(wasm: &mut Reader) -> Result<Vec<Element>, ValidationError> {
        wasm.read_vec(|r| Element::read(r))
    }
}

pub struct CodeSection;

impl CodeSection {
    pub fn read<const N: usize>(
        wasm: &mut Reader,
        builder: &mut CodeBuilder<N>,
        module: &mut Module,
    ) -> Result<(), ValidationError> {
        let n = wasm.read_u32()?;
        if n as usize != module.functions.len() {
            return Err(ValidationError::InvalidCodeSectionFunctionCount);
        }

        for i in 0..n as usize {
            module.read_function_code(wasm, builder, i)?;
        }

        Ok(())
    }
}

pub struct Data {
    pub mem: MemIdx,
    pub offset: Value,
    pub init: Vec<u8>,
}

impl Data {
    pub fn read(wasm: &mut Reader) -> Result<Self, ValidationError> {
        let mem = MemIdx::read(wasm)?;
        let offset = Expr::read_constant(wasm)?;
        let init = wasm.read_vec(|w| w.read_u8())?;

        Ok(Data { mem, offset, init })
    }
}

pub struct DataSection;
impl DataSection {
    pub fn read(wasm: &mut Reader) -> Result<Vec<Data>, ValidationError> {
        wasm.read_vec(|r| Data::read(r))
    }
}
