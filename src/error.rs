use crate::alloc::AllocError;
use crate::SectionTy;
use core::str::Utf8Error;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub offset: u32,
    pub err: SectionError,
}

impl ParseError {
    pub fn new(offset: u32, err: SectionError) -> Self {
        Self { offset, err }
    }
}

#[derive(Debug, Clone)]
pub struct SectionError {
    pub section: Option<SectionTy>,
    pub err: ValidationError,
}

impl SectionError {
    pub fn new_with_section(section: SectionTy, err: ValidationError) -> SectionError {
        SectionError {
            section: Some(section),
            err,
        }
    }

    pub fn new(err: ValidationError) -> SectionError {
        SectionError { section: None, err }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    MalformedMagic([u8; 4]),
    MalformedVersion([u8; 4]),
    MalformedVariableLengthInteger,
    I33IsNegative,
    MalformedUtf8(Utf8Error),
    Eof,
    MalformedSectionId(u8),
    MalformedValueType(u8),
    MalformedFunction(u8),
    MalformedLimit(u8),
    MalformedElemType(u8),
    ExpectedConstOrVar(u8),
    MalformedImportExportDesc(u8),
    InitVecTooLarge(u32),
    AllocationFailure(AllocError),
    InvalidSectionOrdering(SectionTy, SectionTy),
    DuplicateSection(SectionTy),
}

impl From<AllocError> for ValidationError {
    fn from(value: AllocError) -> Self {
        ValidationError::AllocationFailure(value)
    }
}

impl From<AllocError> for SectionError {
    fn from(value: AllocError) -> Self {
        Self::new(ValidationError::AllocationFailure(value))
    }
}

impl From<ValidationError> for SectionError {
    fn from(value: ValidationError) -> Self {
        Self::new(value)
    }
}

impl ValidationError {
    pub fn with_section(self, section: SectionTy) -> SectionError {
        SectionError::new_with_section(section, self)
    }
}
