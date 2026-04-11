use crate::alloc::AllocError;
use crate::SectionTy;
use core::str::Utf8Error;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub offset: u32,
    pub err: SectionDecodeError,
}

impl ParseError {
    pub fn new(offset: u32, err: SectionDecodeError) -> Self {
        Self { offset, err }
    }
}

#[derive(Debug, Clone)]
pub struct SectionDecodeError {
    pub section: Option<SectionTy>,
    pub err: DecodeError,
}

impl SectionDecodeError {
    pub fn new_with_section(section: SectionTy, err: DecodeError) -> SectionDecodeError {
        SectionDecodeError {
            section: Some(section),
            err,
        }
    }

    pub fn new(err: DecodeError) -> SectionDecodeError {
        SectionDecodeError { section: None, err }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
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
    InvalidSectionSize { read: u32, expected: u32 },
}

impl From<AllocError> for DecodeError {
    fn from(value: AllocError) -> Self {
        DecodeError::AllocationFailure(value)
    }
}

impl From<AllocError> for SectionDecodeError {
    fn from(value: AllocError) -> Self {
        Self::new(DecodeError::AllocationFailure(value))
    }
}

impl From<DecodeError> for SectionDecodeError {
    fn from(value: DecodeError) -> Self {
        Self::new(value)
    }
}

impl DecodeError {
    pub fn with_section(self, section: SectionTy) -> SectionDecodeError {
        SectionDecodeError::new_with_section(section, self)
    }
}
