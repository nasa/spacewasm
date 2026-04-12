extern crate std;
use crate::*;
use core::marker::PhantomData;

pub struct Module<'wasm> {
    pub custom: Vec<CustomSection<'wasm>>,
    pub types: Vec<FuncType>,
    pub functions: Vec<TypeIdx>,
    pub code: Vec<Func<'wasm>>,

    pub tables: Vec<TableType>,
    pub memories: Vec<MemType>,
    pub globals: Vec<Global<'wasm>>,
    pub imports: Vec<Import<'wasm>>,
    pub exports: Vec<Export<'wasm>>,
    pub elements: Vec<Element<'wasm>>,
    pub data: Vec<Data<'wasm>>,
    pub start: Option<FuncIdx>,

    // We need to keep this lifetime since we are tracking offsets as `u32` rather
    // than platform dependent `&'wasm` references. This will keep the same outer
    // borrow checking guarentees that we'd get if we tracked true references.
    _marker: PhantomData<&'wasm ()>,
}

impl<'wasm> Module<'wasm> {
    pub fn new(raw: &'wasm [u8]) -> Result<Module<'wasm>, ParseError> {
        let mut wasm = WasmReader::new(raw);
        let start = wasm.save();

        Module::read(&mut wasm).map_err(|err| ParseError {
            offset: wasm.save() - start,
            err: err.into(),
        })
    }

    fn read(wasm: &mut WasmReader<'wasm>) -> Result<Module<'wasm>, SectionDecodeError> {
        let magic = wasm.strip_bytes::<4>()?;
        if magic != [0x00, 0x61, 0x73, 0x6D] {
            return Err(DecodeError::MalformedMagic(magic).into());
        }

        let version = wasm.strip_bytes::<4>()?;

        if version != [0x01, 0x00, 0x00, 0x00] {
            return Err(DecodeError::MalformedVersion(version).into());
        }

        // We need to do a single set of vector allocations per-section
        // First we need to traverse the sections
        let data_start = wasm.save();

        let mut n_custom = 0u32;
        let mut types: Vec<FuncType> = Vec::zero();
        let mut functions: Vec<TypeIdx> = Vec::zero();
        let mut tables: Vec<TableType> = Vec::zero();
        let mut memories: Vec<MemType> = Vec::zero();
        let mut globals: Vec<Global> = Vec::zero();
        let mut imports: Vec<Import> = Vec::zero();
        let mut exports: Vec<Export> = Vec::zero();
        let mut elements: Vec<Element> = Vec::zero();
        let mut code: Vec<Func> = Vec::zero();
        let mut data: Vec<Data> = Vec::zero();

        let mut start: Option<FuncIdx> = None;

        let mut last_section: SectionKind = SectionKind::Custom;

        loop {
            use SectionKind::*;
            let section_ty = match SectionKind::read(wasm) {
                Ok(section) => section,
                Err(DecodeError::Eof) => break,
                Err(e) => return Err(e.into()),
            };

            // Validate the section ordering
            // Custom sections can be interspersed as needed
            if section_ty != Custom {
                if last_section > section_ty && last_section != Custom {
                    return Err(
                        DecodeError::InvalidSectionOrdering(last_section, section_ty).into(),
                    );
                } else if last_section == section_ty {
                    return Err(DecodeError::DuplicateSection(section_ty).into());
                }

                last_section = section_ty;
            }

            let section_size = wasm.read_u32()?;
            let section_start = wasm.save();

            std::eprintln!("reading section {:?}", section_ty);

            match section_ty {
                Custom => {
                    // Count the custom section and skip over them for now
                    // We have nowhere to store them
                    wasm.skip(section_size as usize)
                        .map_err(|e| e.with_section(section_ty))?;
                    n_custom += 1;
                }
                Type => {
                    types = TypeSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                }
                Import => {
                    imports = ImportSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                }
                Function => {
                    functions =
                        FunctionSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                }
                Table => {
                    tables = TableSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                }
                Memory => {
                    memories = MemorySection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                }
                Global => {
                    // globals = GlobalSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                    wasm.skip(section_size as usize)
                        .map_err(|e| e.with_section(section_ty))?;
                }
                Export => {
                    exports = ExportSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                }
                Start => {
                    start.replace(FuncIdx::read(wasm).map_err(|e| e.with_section(section_ty))?);
                }
                Element => {
                    // elements =
                    //     ElementSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                    wasm.skip(section_size as usize)
                        .map_err(|e| e.with_section(section_ty))?;
                }
                Code => {
                    code = CodeSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                }
                Data => {
                    // data = DataSection::read(wasm).map_err(|e| e.with_section(section_ty))?;
                    wasm.skip(section_size as usize)
                        .map_err(|e| e.with_section(section_ty))?;
                }
            }

            let section_end = wasm.save();
            let section_length = section_end - section_start;
            if section_length != section_size {
                return Err(DecodeError::InvalidSectionSize {
                    read: section_length,
                    expected: section_size,
                }
                .with_section(section_ty));
            }
        }

        // Now that we know how many custom sections there are, we can load them into a vector
        let mut custom = Vec::new(n_custom)?;

        wasm.restore(data_start);
        loop {
            use SectionKind::*;
            let section_ty = match SectionKind::read(wasm) {
                Ok(section) => section,
                Err(DecodeError::Eof) => break,
                Err(e) => return Err(e.into()),
            };

            let section_size = wasm.read_u32()?;
            if section_ty == Custom {
                custom.push(
                    CustomSection::read(wasm, section_size)
                        .map_err(|e| e.with_section(section_ty))?,
                );
            } else {
                // Skip over this section
                // We already processed it
                wasm.skip(section_size as usize)
                    .map_err(|e| e.with_section(section_ty))?;
            }
        }

        Ok(Module {
            custom,
            types,
            functions,
            code,
            tables,
            memories,
            globals,
            imports,
            exports,
            elements,
            data,
            start,
            _marker: Default::default(),
        })
    }
}

/// All WASM sections ordered by the order expected in the file
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
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
}

impl SectionKind {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        use SectionKind::*;
        let ty = match wasm.read_u8()? {
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
            other => return Err(DecodeError::MalformedSectionId(other)),
        };

        Ok(ty)
    }
}

pub struct CustomSection<'wasm> {
    pub name: Name<'wasm>,
    pub data: Slice<'wasm>,
}

impl<'wasm> CustomSection<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>, size: u32) -> Result<Self, DecodeError> {
        let start = wasm.save();
        let name = Name::read(wasm)?;
        let name_length = wasm.save() - start;

        let data = Slice::read(wasm, size - name_length)?;

        Ok(CustomSection { name, data })
    }
}

pub struct TypeSection;

impl TypeSection {
    pub fn read(wasm: &mut WasmReader) -> Result<Vec<FuncType>, DecodeError> {
        wasm.read_vec(FuncType::read)
    }
}

macro_rules! read_impl_u32 {
    ($type_name:ident) => {
        #[derive(Debug, Clone)]
        pub struct $type_name(pub u32);
        impl $type_name {
            pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
                Ok($type_name(wasm.read_u32()?))
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
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        match wasm.read_u8()? {
            0x00 => Ok(ImportDesc::Func(TypeIdx::read(wasm)?)),
            0x01 => Ok(ImportDesc::Table(TableType::read(wasm)?)),
            0x02 => Ok(ImportDesc::Mem(MemType::read(wasm)?)),
            0x03 => Ok(ImportDesc::Global(GlobalType::read(wasm)?)),
            c => Err(DecodeError::MalformedImportExportDesc(c)),
        }
    }
}

pub enum ExportDesc {
    Func(FuncIdx),
    Table(TableIdx),
    Mem(MemIdx),
    Global(GlobalIdx),
}

impl ExportDesc {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        match wasm.read_u8()? {
            0x00 => Ok(ExportDesc::Func(FuncIdx::read(wasm)?)),
            0x01 => Ok(ExportDesc::Table(TableIdx::read(wasm)?)),
            0x02 => Ok(ExportDesc::Mem(MemIdx::read(wasm)?)),
            0x03 => Ok(ExportDesc::Global(GlobalIdx::read(wasm)?)),
            c => Err(DecodeError::MalformedImportExportDesc(c)),
        }
    }
}

pub struct Import<'wasm> {
    pub module: Name<'wasm>,
    pub name: Name<'wasm>,
    pub desc: ImportDesc,
}

impl<'wasm> Import<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, DecodeError> {
        let module = Name::read(wasm)?;
        let name = Name::read(wasm)?;
        let desc = ImportDesc::read(wasm)?;
        Ok(Import { module, name, desc })
    }
}

pub struct ImportSection;

impl ImportSection {
    pub fn read<'wasm>(wasm: &mut WasmReader<'wasm>) -> Result<Vec<Import<'wasm>>, DecodeError> {
        wasm.read_vec(Import::read)
    }
}

pub struct FunctionSection;

impl FunctionSection {
    pub fn read(wasm: &mut WasmReader) -> Result<Vec<TypeIdx>, DecodeError> {
        wasm.read_vec(TypeIdx::read)
    }
}

pub struct TableSection;

impl TableSection {
    pub fn read(wasm: &mut WasmReader) -> Result<Vec<TableType>, DecodeError> {
        wasm.read_vec(TableType::read)
    }
}

pub struct MemorySection;
impl MemorySection {
    pub fn read(wasm: &mut WasmReader) -> Result<Vec<MemType>, DecodeError> {
        wasm.read_vec(MemType::read)
    }
}

pub struct Expr<'wasm> {
    pub instructions: Slice<'wasm>,
}
impl<'wasm> Expr<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, DecodeError> {
        // TODO(tumbar) Decode instructions
        Ok(Expr {
            instructions: Slice::read(wasm, 0)?,
        })
    }
}

pub struct Global<'wasm> {
    pub type_: GlobalType,
    pub init: Expr<'wasm>,
}

impl<'wasm> Global<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, DecodeError> {
        let type_ = GlobalType::read(wasm)?;
        let init = Expr::read(wasm)?;
        Ok(Global { type_, init })
    }
}

pub struct GlobalSection;
impl GlobalSection {
    pub fn read<'wasm>(wasm: &mut WasmReader<'wasm>) -> Result<Vec<Global<'wasm>>, DecodeError> {
        wasm.read_vec(Global::read)
    }
}

pub struct Export<'wasm> {
    pub name: Name<'wasm>,
    pub desc: ExportDesc,
}

impl<'wasm> Export<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, DecodeError> {
        let name = Name::read(wasm)?;
        let desc = ExportDesc::read(wasm)?;
        Ok(Export { name, desc })
    }
}

pub struct ExportSection;
impl ExportSection {
    pub fn read<'wasm>(wasm: &mut WasmReader<'wasm>) -> Result<Vec<Export<'wasm>>, DecodeError> {
        wasm.read_vec(Export::read)
    }
}

pub struct Element<'wasm> {
    pub table: TableIdx,
    pub offset: Expr<'wasm>,
    pub init: Vec<FuncIdx>,
}

impl<'wasm> Element<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, DecodeError> {
        let table = TableIdx::read(wasm)?;
        let offset = Expr::read(wasm)?;
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
    pub fn read<'wasm>(wasm: &mut WasmReader<'wasm>) -> Result<Vec<Element<'wasm>>, DecodeError> {
        wasm.read_vec(Element::read)
    }
}

pub struct CodeSection;

impl CodeSection {
    pub fn read<'wasm>(wasm: &mut WasmReader<'wasm>) -> Result<Vec<Func<'wasm>>, DecodeError> {
        wasm.read_vec(Func::read)
    }
}

pub struct Data<'wasm> {
    pub mem: MemIdx,
    pub offset: Expr<'wasm>,
    pub init: Slice<'wasm>,
}

impl<'wasm> Data<'wasm> {
    pub fn read(wasm: &mut WasmReader<'wasm>) -> Result<Self, DecodeError> {
        let mem = MemIdx::read(wasm)?;
        let offset = Expr::read(wasm)?;

        let init_n = wasm.read_u32()?;
        let init = Slice::read(wasm, init_n)?;

        Ok(Data { mem, offset, init })
    }
}

pub struct DataSection;
impl DataSection {
    pub fn read<'wasm>(wasm: &mut WasmReader<'wasm>) -> Result<Vec<Data<'wasm>>, DecodeError> {
        wasm.read_vec(Data::read)
    }
}
