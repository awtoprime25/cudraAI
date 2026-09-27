use cudra_span::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    NoneTy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: Ty,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub id: FuncId,
    pub name: String,
    pub params: Vec<LocalId>,
    pub locals: Vec<Local>,
    pub ret: Ty,
    pub body: Vec<TStmt>,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub funcs: Vec<Func>,
    pub main: FuncId,
}

#[derive(Clone, Debug)]
pub enum TStmt {
    Store {
        local: LocalId,
        value: TExpr,
    },
    // start/stop/step avaliados uma vez, nesta ordem; a regra 14 (variável
    // congelada por iteração, valor final depois do ciclo) é do codegen.
    ForRange {
        var: LocalId,
        start: TExpr,
        stop: TExpr,
        step: TExpr,
        body: Vec<TStmt>,
    },
    While {
        cond: TExpr,
        body: Vec<TStmt>,
    },
    If {
        cond: TExpr,
        then: Vec<TStmt>,
        orelse: Vec<TStmt>,
    },
    Return(Option<TExpr>),
    Break,
    Continue,
    Expr(TExpr),
}

#[derive(Clone, Debug)]
pub struct TExpr {
    pub ty: Ty,
    pub kind: TExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TExprKind {
    ConstInt(i64),
    ConstFloat(f64),
    ConstBool(bool),
    ConstNone,
    Local(LocalId),
    Binary {
        op: TBinOp,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    Unary {
        op: TUnOp,
        operand: Box<TExpr>,
    },
    // cadeia `a < b < c`: cada operando avaliado uma vez, esquerda→direita,
    // com curto-circuito. As comparações mistas usam IntFloat/FloatInt (sem
    // Cast: são exatas, regra 12).
    Compare {
        first: Box<TExpr>,
        rest: Vec<(TCmp, TExpr)>,
    },
    And(Box<TExpr>, Box<TExpr>),
    Or(Box<TExpr>, Box<TExpr>),
    // destino = self.ty. Pares válidos: int<->float, bool->int, bool->float,
    // int->bool, float->bool.
    Cast {
        value: Box<TExpr>,
    },
    Call {
        func: FuncId,
        args: Vec<TExpr>,
    },
    Intrinsic {
        which: Intrinsic,
        args: Vec<TExpr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TBinOp {
    IAdd,
    ISub,
    IMul,
    ITrueDiv,
    IFloorDiv,
    IMod,
    IPow,
    IAnd,
    IOr,
    IXor,
    IShl,
    IShr,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FFloorDiv,
    FMod,
    FPow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TUnOp {
    INeg,
    FNeg,
    IInvert,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TCmp {
    I(CmpOp),
    F(CmpOp),
    B(CmpOp),
    IntFloat(CmpOp),
    FloatInt(CmpOp),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intrinsic {
    Print,
    AbsInt,
    AbsFloat,
    ArgInt,
    ArgFloat,
}

mod verify;
pub use verify::verify;

mod print;
pub use print::print_module;
