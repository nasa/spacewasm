use crate::alloc::AllocError;
use crate::SectionKind;

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
    pub section: Option<SectionKind>,
    pub err: ValidationError,
}

impl SectionDecodeError {
    pub fn new_with_section(section: SectionKind, err: ValidationError) -> SectionDecodeError {
        SectionDecodeError {
            section: Some(section),
            err,
        }
    }

    pub fn new(err: ValidationError) -> SectionDecodeError {
        SectionDecodeError { section: None, err }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    Eof,
    MalformedVariableLengthInteger,
    I33IsNegative,
    AllocError(AllocError),
    MalformedMagic,
    MalformedVersion,
    MalformedUtf8,
    MalformedSectionId(u8),
    MalformedValueType(u8),
    MalformedFunction(u8),
    MalformedLimit(u8),
    MalformedElemType(u8),
    MalformedSectionSize,
    ExpectedConstOrVar(u8),
    MalformedImportExportDesc(u8),
    AllocationFailure(AllocError),
    InvalidSectionOrdering(SectionKind, SectionKind),
    DuplicateSection(SectionKind),
    InvalidZeroMaxLimit,
    ExpectedTerminal(u8),
    InvalidOpcode(u8),
    MalformedCodeSize,
    VecTooLong,
}

impl From<AllocError> for ValidationError {
    fn from(value: AllocError) -> Self {
        ValidationError::AllocationFailure(value)
    }
}

impl From<AllocError> for SectionDecodeError {
    fn from(value: AllocError) -> Self {
        Self::new(ValidationError::AllocationFailure(value))
    }
}

impl From<ValidationError> for SectionDecodeError {
    fn from(value: ValidationError) -> Self {
        Self::new(value)
    }
}

impl ValidationError {
    pub fn with_section(self, section: SectionKind) -> SectionDecodeError {
        SectionDecodeError::new_with_section(section, self)
    }
}
