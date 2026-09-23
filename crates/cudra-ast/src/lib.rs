use cudra_span::{Span, Symbol};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: Symbol,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub enum Item {
    Func(FuncDef),
    Stmt(Stmt), // instruções de topo → main() implícito
}

#[derive(Clone, Debug)]
pub struct FuncDef {
    pub id: NodeId,
    pub name: Ident,
    pub decorators: Vec<Expr>,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>, // `-> int` opcional
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub id: NodeId,
    pub name: Ident,
    pub ty: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub id: NodeId,
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Assign {
        target: Target,
        ty: Option<TypeExpr>,
        value: Expr,
    }, // x = 5 · x: f32 = 5
    AugAssign {
        target: Target,
        op: BinOp,
        value: Expr,
    }, // x += 1
    For {
        var: Ident,
        iter: Expr,
        body: Block,
    }, // iter = range(...) | gpu.range(...)
    While {
        cond: Expr,
        body: Block,
    },
    If {
        cond: Expr,
        then: Block,
        elifs: Vec<(Expr, Block)>,
        orelse: Option<Block>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Pass,
    Expr(Expr), // print(x), f(y)
    Import {
        module: Ident,
    }, // import cudra
    FromImport {
        module: Ident,
        names: Vec<Ident>,
    }, // from cudra import arg_int
}

#[derive(Clone, Debug)]
pub enum Target {
    Name(Ident),
    Index { base: Box<Expr>, index: Box<Expr> }, // y[i] = ...
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(u64), // o sinal vem de Unary::Neg; o intervalo é validado na sema
    Float(f64),
    Bool(bool),
    NoneLit,
    Name(Ident),
    Unary {
        op: UnOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Logical {
        op: LogicOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    }, // and/or com curto-circuito
    Compare {
        first: Box<Expr>,
        rest: Vec<(CmpOp, Expr)>,
    }, // a < b <= c
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Attr {
        base: Box<Expr>,
        attr: Ident,
    }, // gpu.range, math.sqrt
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
}

#[derive(Clone, Debug)]
pub struct TypeExpr {
    pub id: NodeId,
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeExprKind {
    Name(Ident),                                  // int, f32
    Generic { base: Ident, args: Vec<TypeExpr> }, // Array[f32]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
    Not,
    Invert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicOp {
    And,
    Or,
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
