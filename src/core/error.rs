use crate::alloc::AllocError;
use crate::core::constant::ConstantExprError;
use crate::core::ReaderError;
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
    MalformedInteger,
    I33IsNegative,
    MalformedMagic,
    MalformedVersion,
    MalformedUtf8,
    DuplicateModuleName,
    MalformedSectionId(u8),
    MalformedValueType(u8),
    MalformedFunction(u8),
    MalformedLimit(u8),
    MalformedElemType(u8),
    MalformedSectionSize,
    ExpectedConstOrVar(u8),
    MalformedImportExportDesc(u8),
    InvalidSectionOrdering(SectionKind, SectionKind),
    DuplicateSection(SectionKind),
    InvalidMaxLimit,
    ExpectedTerminal(u8),
    InvalidOpcode(u8),
    MalformedCodeSize,
    InvalidCodeSectionFunctionCount,
    VecTooLong,
    IdxTooLarge,
    ModuleIdxTooLarge,
    MemAlignTooLarge,
    ControlFlowTooDeep,
    StackUnderflow,
    StackTooLarge,
    LabelStackJumpTooDeep,
    LabelJumpTooLarge,
    TypeMismatch,
    BlockResultTypeMismatch,
    IllegalMemoryGrow,
    InvalidElementOffset,
    InvalidElementOutOfBounds,
    InvalidTableIndex,
    InvalidMemIndex,
    InvalidMemOffsetType,
    InvalidNegativeMemOffset,
    InvalidMemOffset,
    InvalidLabelIndex,
    InvalidElseBlock,
    InvalidEndBlock,
    MultipleMemories,
    PossibleBackpatchCycle,
    PageFault,
    InstructionOutsideOfFunction,
    LocalIdxOutOfRange,
    FunctionIdxOutOfRange,
    TypeIdxOutOfRange,
    FunctionTextOutOfRange,
    GlobalIdxOutOfRange,
    FunctionImportNotFound,
    GlobalImportNotFound,
    FunctionImportOutOfRange,
    FunctionImportTypeMismatch,
    GlobalIsNotMutable,
    GlobalImportTypeMismatch,
    FunctionParametersTooLarge,
    FunctionReturnsTooLarge,
    TooManyLocals,
    InvalidConstInstruction,
    BrTableHasTooManyCases,
    GlobalTypeMismatch,
    AlignmentLargerThanType,
    TableImportsNotSupportedYet, // TODO(tumbar) Implement dynamic linking
    MemoryImportsNotSupportedYet, // TODO(tumbar) Implement implement shared memory
    FunctionCallsAcrossModuleNotSupportedYet, // TODO(tumbar) Implement module context isolation
    GlobalsAcrossModuleNotSupportedYet, // TODO(tumbar) Implement module context isolation
    InvalidConstantExpr(ConstantExprError),
    AllocError(AllocError),
    ReaderError(ReaderError),
}

impl From<AllocError> for ValidationError {
    fn from(value: AllocError) -> Self {
        ValidationError::AllocError(value)
    }
}

impl From<ConstantExprError> for ValidationError {
    fn from(value: ConstantExprError) -> Self {
        ValidationError::InvalidConstantExpr(value)
    }
}

impl From<AllocError> for SectionDecodeError {
    fn from(value: AllocError) -> Self {
        Self::new(ValidationError::AllocError(value))
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
