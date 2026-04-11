use crate::*;

#[derive(Default)]
#[repr(C)]
pub struct Statistics {
    pub custom: u32,
    pub types: u32,
    pub functions: u32,
    pub tables: u32,
    pub memories: u32,
    pub globals: u32,
    pub imports: u32,
    pub exports: u32,
    pub elements: u32,
}

pub struct Module {
    pub custom: Vec<CustomSection>,
    pub types: Vec<FuncType>,
    pub functions: Vec<TypeIdx>,
    pub tables: Vec<TableType>,
    pub memories: Vec<MemType>,
    pub globals: Vec<Global>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
    pub elements: Vec<Element>,
    pub start: Option<FuncIdx>,
}

impl Module {
    pub fn new(raw: &[u8]) -> Result<Module, ParseError> {
        let mut wasm = WasmReader::new(raw);
        let start = wasm.save();

        Module::read(&mut wasm).map_err(|err| ParseError {
            offset: wasm.save() - start,
            err: err.into(),
        })
    }

    fn read(wasm: &mut WasmReader) -> Result<Module, SectionDecodeError> {
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

        // pub custom: Vec<CustomSection>,
        let mut n_custom = 0u32;
        let mut types: Option<Vec<FuncType>> = None;
        let mut functions: Option<Vec<TypeIdx>> = None;
        let mut tables: Option<Vec<TableType>> = None;
        let mut memories: Option<Vec<MemType>> = None;
        let mut globals: Option<Vec<Global>> = None;
        let mut imports: Option<Vec<Import>> = None;
        let mut exports: Option<Vec<Export>> = None;
        let mut elements: Option<Vec<Element>> = None;
        let mut start: Option<FuncIdx> = None;

        let mut last_section: SectionTy = SectionTy::Custom;

        loop {
            use SectionTy::*;
            let section_ty = match SectionTy::read(wasm) {
                Ok(section) => section,
                Err(DecodeError::Eof) => {
                    break;
                }
                Err(e) => return Err(e.into()),
            };

            // Validate the section ordering
            // Custom sections can be interspersed as needed
            if section_ty != Custom && last_section != Custom {
                if last_section > section_ty {
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

            match section_ty {
                Custom => {
                    // Count the custom section and skip over them for now
                    // We have nowhere to store them
                    let _ = wasm
                        .read_n(section_size as usize)
                        .map_err(|e| e.with_section(section_ty))?;
                    n_custom += 1;
                }
                Type => {
                    types.replace(TypeSection::read(wasm).map_err(|e| e.with_section(section_ty))?);
                }
                Import => {
                    imports.replace(
                        ImportSection::read(wasm).map_err(|e| e.with_section(section_ty))?,
                    );
                }
                Function => {
                    functions.replace(
                        FunctionSection::read(wasm).map_err(|e| e.with_section(section_ty))?,
                    );
                }
                Table => {
                    tables
                        .replace(TableSection::read(wasm).map_err(|e| e.with_section(section_ty))?);
                }
                Memory => {
                    memories.replace(
                        MemorySection::read(wasm).map_err(|e| e.with_section(section_ty))?,
                    );
                }
                Global => {
                    globals.replace(
                        GlobalSection::read(wasm).map_err(|e| e.with_section(section_ty))?,
                    );
                }
                Export => {
                    exports.replace(
                        ExportSection::read(wasm).map_err(|e| e.with_section(section_ty))?,
                    );
                }
                Start => {
                    start.replace(FuncIdx::read(wasm).map_err(|e| e.with_section(section_ty))?);
                }
                Element => {
                    elements.replace(
                        ElementSection::read(wasm).map_err(|e| e.with_section(section_ty))?,
                    );
                }
                Code => {
                    // stats.code += 1;
                }
                Data => {}
                DataCount => {}
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
            use SectionTy::*;
            let section_ty = match SectionTy::read(wasm) {
                Ok(section) => section,
                Err(DecodeError::Eof) => {
                    break;
                }
                Err(e) => return Err(e.into()),
            };

            let section_size = wasm.read_u32()?;
            if section_ty != Custom {
                custom.push(
                    CustomSection::read(wasm, section_size as usize)
                        .map_err(|e| e.with_section(section_ty))?,
                );
            } else {
                // Skip over this section
                // We already processed it
                wasm.read_n(section_size as usize)
                    .map_err(|e| e.with_section(section_ty))?;
            }
        }

        Ok(Module {
            custom,
            types: types.unwrap_or(Vec::zero()),
            functions: functions.unwrap_or(Vec::zero()),
            tables: tables.unwrap_or(Vec::zero()),
            memories: memories.unwrap_or(Vec::zero()),
            globals: globals.unwrap_or(Vec::zero()),
            imports: imports.unwrap_or(Vec::zero()),
            exports: exports.unwrap_or(Vec::zero()),
            elements: elements.unwrap_or(Vec::zero()),
            start,
        })
    }
}

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, Ord, PartialOrd)]
#[repr(u32)]
pub enum SectionTy {
    Custom = 0,
    Type = 1,
    Import = 2,
    Function = 3,
    Table = 4,
    Memory = 5,
    Global = 6,
    Export = 7,
    Start = 8,
    Element = 9,
    Code = 10,
    Data = 11,
    DataCount = 12,
}

impl SectionTy {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        use SectionTy::*;
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
            12 => DataCount,
            other => return Err(DecodeError::MalformedSectionId(other)),
        };

        Ok(ty)
    }
}

#[repr(C)]
pub struct CustomSection {
    pub name: Name,
    pub data: Slice,
}

impl CustomSection {
    pub fn read(wasm: &mut WasmReader, size: usize) -> Result<Self, DecodeError> {
        let start = wasm.save();
        let name = Name::read(wasm)?;
        let name_length = wasm.save() - start;

        let data = Slice::read(wasm, size - name_length as usize)?;

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
        pub struct $type_name(u32);
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

pub enum ImportExportDesc {
    Func(TypeIdx),
    Table(TableType),
    Mem(MemType),
    Global(GlobalType),
}

impl ImportExportDesc {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        match wasm.read_u8()? {
            0x00 => Ok(ImportExportDesc::Func(TypeIdx::read(wasm)?)),
            0x01 => Ok(ImportExportDesc::Table(TableType::read(wasm)?)),
            0x02 => Ok(ImportExportDesc::Mem(MemType::read(wasm)?)),
            0x03 => Ok(ImportExportDesc::Global(GlobalType::read(wasm)?)),
            c => Err(DecodeError::MalformedImportExportDesc(c)),
        }
    }
}

pub struct Import {
    pub module: Name,
    pub name: Name,
    pub desc: ImportExportDesc,
}

impl Import {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        let module = Name::read(wasm)?;
        let name = Name::read(wasm)?;
        let desc = ImportExportDesc::read(wasm)?;
        Ok(Import { module, name, desc })
    }
}

pub struct ImportSection;

impl ImportSection {
    pub fn read(wasm: &mut WasmReader) -> Result<Vec<Import>, DecodeError> {
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

pub struct Expr {
    pub instructions: Slice,
}
impl Expr {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        // TODO(tumbar) Decode instructions
        Ok(Expr {
            instructions: Slice::read(wasm, 0)?,
        })
    }
}

pub struct Global {
    pub type_: GlobalType,
    pub init: Expr,
}

impl Global {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        let type_ = GlobalType::read(wasm)?;
        let init = Expr::read(wasm)?;
        Ok(Global { type_, init })
    }
}

pub struct GlobalSection;
impl GlobalSection {
    pub fn read(wasm: &mut WasmReader) -> Result<Vec<Global>, DecodeError> {
        wasm.read_vec(Global::read)
    }
}

pub struct Export {
    pub name: Name,
    pub desc: ImportExportDesc,
}

impl Export {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
        let name = Name::read(wasm)?;
        let desc = ImportExportDesc::read(wasm)?;
        Ok(Export { name, desc })
    }
}

pub struct ExportSection;
impl ExportSection {
    pub fn read(wasm: &mut WasmReader) -> Result<Vec<Export>, DecodeError> {
        wasm.read_vec(Export::read)
    }
}

pub struct Element {
    pub table: TableIdx,
    pub offset: Expr,
    pub init: Vec<FuncIdx>,
}

impl Element {
    pub fn read(wasm: &mut WasmReader) -> Result<Self, DecodeError> {
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
    pub fn read(wasm: &mut WasmReader) -> Result<Vec<Element>, DecodeError> {
        wasm.read_vec(Element::read)
    }
}
