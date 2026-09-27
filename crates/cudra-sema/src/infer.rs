use cudra_ast as ast;
use cudra_tir::{CmpOp as TCmpOp, TBinOp, TCmp, TUnOp};

use crate::STy;

pub(crate) enum BinOutcome {
    Ok {
        result: STy,
        op: TBinOp,
        promote_left: bool,
        promote_right: bool,
    },
    BoolOperand,
    NoneOperand,
    Invalid,
    Error,
}

pub(crate) fn binop_outcome(op: ast::BinOp, lt: STy, rt: STy) -> BinOutcome {
    if lt == STy::Error || rt == STy::Error {
        return BinOutcome::Error;
    }
    if lt == STy::Bool || rt == STy::Bool {
        return BinOutcome::BoolOperand;
    }
    if lt == STy::NoneTy || rt == STy::NoneTy {
        return BinOutcome::NoneOperand;
    }
    // a partir daqui lt, rt ∈ {Int, Float}
    use ast::BinOp::*;
    match op {
        Add | Sub | Mul | Div | FloorDiv | Mod | Pow => {
            let both_int = lt == STy::Int && rt == STy::Int;
            if both_int && op == Div {
                BinOutcome::Ok {
                    result: STy::Float,
                    op: TBinOp::ITrueDiv,
                    promote_left: false,
                    promote_right: false,
                }
            } else if both_int {
                let iop = match op {
                    Add => TBinOp::IAdd,
                    Sub => TBinOp::ISub,
                    Mul => TBinOp::IMul,
                    FloorDiv => TBinOp::IFloorDiv,
                    Mod => TBinOp::IMod,
                    Pow => TBinOp::IPow,
                    _ => unreachable!(),
                };
                BinOutcome::Ok {
                    result: STy::Int,
                    op: iop,
                    promote_left: false,
                    promote_right: false,
                }
            } else {
                let fop = match op {
                    Add => TBinOp::FAdd,
                    Sub => TBinOp::FSub,
                    Mul => TBinOp::FMul,
                    Div => TBinOp::FDiv,
                    FloorDiv => TBinOp::FFloorDiv,
                    Mod => TBinOp::FMod,
                    Pow => TBinOp::FPow,
                    _ => unreachable!(),
                };
                BinOutcome::Ok {
                    result: STy::Float,
                    op: fop,
                    promote_left: lt == STy::Int,
                    promote_right: rt == STy::Int,
                }
            }
        }
        BitAnd | BitOr | BitXor | Shl | Shr => {
            if lt == STy::Int && rt == STy::Int {
                let iop = match op {
                    BitAnd => TBinOp::IAnd,
                    BitOr => TBinOp::IOr,
                    BitXor => TBinOp::IXor,
                    Shl => TBinOp::IShl,
                    Shr => TBinOp::IShr,
                    _ => unreachable!(),
                };
                BinOutcome::Ok {
                    result: STy::Int,
                    op: iop,
                    promote_left: false,
                    promote_right: false,
                }
            } else {
                BinOutcome::Invalid
            }
        }
    }
}

pub(crate) enum UnOutcome {
    Ok { result: STy, op: Option<TUnOp> },
    BoolOperand,
    NoneOperand,
    Invalid,
    Error,
}

pub(crate) fn unop_outcome(op: ast::UnOp, ot: STy) -> UnOutcome {
    if ot == STy::Error {
        return UnOutcome::Error;
    }
    if ot == STy::Bool {
        return UnOutcome::BoolOperand;
    }
    if ot == STy::NoneTy {
        return UnOutcome::NoneOperand;
    }
    use ast::UnOp::*;
    match op {
        Neg => {
            let t = if ot == STy::Int {
                TUnOp::INeg
            } else {
                TUnOp::FNeg
            };
            UnOutcome::Ok {
                result: ot,
                op: Some(t),
            }
        }
        Pos => UnOutcome::Ok {
            result: ot,
            op: None,
        },
        Invert => {
            if ot == STy::Int {
                UnOutcome::Ok {
                    result: STy::Int,
                    op: Some(TUnOp::IInvert),
                }
            } else {
                UnOutcome::Invalid
            }
        }
        Not => unreachable!("Not é tratado à parte, via veracidade"),
    }
}

pub(crate) enum CmpOutcome {
    Ok(TCmp),
    BoolMismatch,
    NoneOperand,
    Error,
}

fn map_cmp(op: ast::CmpOp) -> TCmpOp {
    match op {
        ast::CmpOp::Eq => TCmpOp::Eq,
        ast::CmpOp::Ne => TCmpOp::Ne,
        ast::CmpOp::Lt => TCmpOp::Lt,
        ast::CmpOp::Le => TCmpOp::Le,
        ast::CmpOp::Gt => TCmpOp::Gt,
        ast::CmpOp::Ge => TCmpOp::Ge,
    }
}

pub(crate) fn cmp_outcome(op: ast::CmpOp, lt: STy, rt: STy) -> CmpOutcome {
    if lt == STy::Error || rt == STy::Error {
        return CmpOutcome::Error;
    }
    if lt == STy::NoneTy || rt == STy::NoneTy {
        return CmpOutcome::NoneOperand;
    }
    let cop = map_cmp(op);
    if lt == STy::Bool || rt == STy::Bool {
        if lt == STy::Bool && rt == STy::Bool && matches!(op, ast::CmpOp::Eq | ast::CmpOp::Ne) {
            return CmpOutcome::Ok(TCmp::B(cop));
        }
        return CmpOutcome::BoolMismatch;
    }
    match (lt, rt) {
        (STy::Int, STy::Int) => CmpOutcome::Ok(TCmp::I(cop)),
        (STy::Float, STy::Float) => CmpOutcome::Ok(TCmp::F(cop)),
        (STy::Int, STy::Float) => CmpOutcome::Ok(TCmp::IntFloat(cop)),
        (STy::Float, STy::Int) => CmpOutcome::Ok(TCmp::FloatInt(cop)),
        _ => unreachable!(),
    }
}
