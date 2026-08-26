/// How tightly an operator binds. Higher wins!
#[derive(Debug, Eq, Ord, PartialOrd, PartialEq, Copy, Clone, Hash)]
pub(crate) enum BindingPower {
    Lowest = 0,
    UnaryOrBinaryHelp = 1,
    FunctionOrLoopOrIf = 2,
    BinaryLeftAssign = 3,
    BinaryEqualsAssign = 4,
    BinaryRightAssign = 5,
    UnaryOrBinaryTilde = 6,
    BinaryOr = 7,
    BinaryAnd = 8,
    UnaryNot = 9,
    BinaryComparison = 10,
    BinaryPlusMinus = 11,
    BinaryMultiplyDivide = 12,
    BinarySpecialOrPipe = 13,
    BinaryColon = 14,
    UnaryPlusMinus = 15,
    BinaryExponentiate = 16,
    Highest = 17,
}

impl BindingPower {
    /// Used by left associative constructs
    pub(crate) fn increment(self) -> Self {
        match self {
            Self::Lowest => Self::UnaryOrBinaryHelp,
            Self::UnaryOrBinaryHelp => Self::FunctionOrLoopOrIf,
            Self::FunctionOrLoopOrIf => Self::BinaryLeftAssign,
            Self::BinaryLeftAssign => Self::BinaryEqualsAssign,
            Self::BinaryEqualsAssign => Self::BinaryRightAssign,
            Self::BinaryRightAssign => Self::UnaryOrBinaryTilde,
            Self::UnaryOrBinaryTilde => Self::BinaryOr,
            Self::BinaryOr => Self::BinaryAnd,
            Self::BinaryAnd => Self::UnaryNot,
            Self::UnaryNot => Self::BinaryComparison,
            Self::BinaryComparison => Self::BinaryPlusMinus,
            Self::BinaryPlusMinus => Self::BinaryMultiplyDivide,
            Self::BinaryMultiplyDivide => Self::BinarySpecialOrPipe,
            Self::BinarySpecialOrPipe => Self::BinaryColon,
            Self::BinaryColon => Self::UnaryPlusMinus,
            Self::UnaryPlusMinus => Self::BinaryExponentiate,
            Self::BinaryExponentiate => Self::Highest,
            Self::Highest => unreachable!("Highest is a sentinel, never incremented"),
        }
    }
}
