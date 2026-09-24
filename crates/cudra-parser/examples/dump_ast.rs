use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;
use std::thread;

use cudra_ast::{
    BinOp, CmpOp, Expr, ExprKind, FuncDef, Item, LogicOp, Stmt, StmtKind, Target, TypeExpr,
    TypeExprKind, UnOp,
};
use cudra_lexer::lex;
use cudra_parser::parse;
use cudra_span::{Interner, SourceMap};

fn main() -> ExitCode {
    let builder = thread::Builder::new().stack_size(256 << 20);
    match builder.spawn(run) {
        Ok(h) => h.join().unwrap_or(ExitCode::from(2)),
        Err(_) => ExitCode::from(2),
    }
}

fn run() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("uso: dump_ast <ficheiro>");
        return ExitCode::from(1);
    };

    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            println!("ERROR\tnão foi possível ler o ficheiro: {e}\t1:1");
            return ExitCode::from(1);
        }
    };

    let src = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            let pos = e.utf8_error().valid_up_to();
            let byte = e.as_bytes()[pos];
            println!(
                "ERROR\tficheiro não é UTF-8 válido (byte 0x{byte:02X} na posição {pos})\t1:1"
            );
            return ExitCode::from(1);
        }
    };

    let mut sources = SourceMap::new();
    let file = sources.add(path.clone(), src.clone());
    let (tokens, lex_diags) = lex(file, &src);

    let mut interner = Interner::new();
    let (module, parse_diags) = parse(file, &src, &tokens, &mut interner);

    let use_lex_errors = !lex_diags.is_empty();
    let diags = if use_lex_errors {
        &lex_diags
    } else {
        &parse_diags
    };

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if diags.is_empty() {
        for it in &module.items {
            if writeln!(out, "{}", item_sexpr(it, &interner)).is_err() {
                return ExitCode::SUCCESS;
            }
        }
    }
    for diag in diags {
        let (line, col) = diag
            .labels
            .first()
            .map(|l| sources.line_col(file, l.span.lo))
            .unwrap_or((1, 1));
        if writeln!(out, "ERROR\t{}\t{line}:{col}", diag.message).is_err() {
            return ExitCode::SUCCESS;
        }
    }
    let _ = out.flush();

    if diags.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn wrap(name: &str, inner: &str) -> String {
    if inner.is_empty() {
        format!("({name})")
    } else {
        format!("({name} {inner})")
    }
}

fn bin_op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::FloorDiv => "floordiv",
        BinOp::Mod => "mod",
        BinOp::Pow => "pow",
        BinOp::BitAnd => "bitand",
        BinOp::BitOr => "bitor",
        BinOp::BitXor => "bitxor",
        BinOp::Shl => "shl",
        BinOp::Shr => "shr",
    }
}

fn un_op_str(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "neg",
        UnOp::Pos => "pos",
        UnOp::Not => "not",
        UnOp::Invert => "invert",
    }
}

fn cmp_op_str(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "eq",
        CmpOp::Ne => "ne",
        CmpOp::Lt => "lt",
        CmpOp::Le => "le",
        CmpOp::Gt => "gt",
        CmpOp::Ge => "ge",
    }
}

fn float_hex(v: f64) -> String {
    format!("{:016x}", v.to_bits())
}

fn flatten_bool(op: LogicOp, e: &Expr, interner: &Interner, out: &mut Vec<String>) {
    if let ExprKind::Logical { op: op2, lhs, rhs } = &e.kind {
        if *op2 == op {
            flatten_bool(op, lhs, interner, out);
            flatten_bool(op, rhs, interner, out);
            return;
        }
    }
    out.push(ex(e, interner));
}

fn ex(e: &Expr, interner: &Interner) -> String {
    match &e.kind {
        ExprKind::Int(v) => format!("(int {v})"),
        ExprKind::Float(v) => format!("(float {})", float_hex(*v)),
        ExprKind::Bool(b) => format!("(bool {})", if *b { "True" } else { "False" }),
        ExprKind::NoneLit => "(none)".to_string(),
        ExprKind::Name(id) => format!("(name {})", interner.resolve(id.name)),
        ExprKind::Unary { op, operand } => {
            format!("(unary {} {})", un_op_str(*op), ex(operand, interner))
        }
        ExprKind::Binary { op, lhs, rhs } => format!(
            "(bin {} {} {})",
            bin_op_str(*op),
            ex(lhs, interner),
            ex(rhs, interner)
        ),
        ExprKind::Logical { op, lhs, rhs } => {
            let mut parts = Vec::new();
            flatten_bool(*op, lhs, interner, &mut parts);
            flatten_bool(*op, rhs, interner, &mut parts);
            let name = match op {
                LogicOp::And => "and",
                LogicOp::Or => "or",
            };
            format!("({name} {})", parts.join(" "))
        }
        ExprKind::Compare { first, rest } => {
            let mut parts = vec![ex(first, interner)];
            for (op, c) in rest {
                parts.push(cmp_op_str(*op).to_string());
                parts.push(ex(c, interner));
            }
            format!("(cmp {})", parts.join(" "))
        }
        ExprKind::Call { callee, args } => {
            let mut parts = vec![ex(callee, interner)];
            for a in args {
                parts.push(ex(a, interner));
            }
            format!("(call {})", parts.join(" "))
        }
        ExprKind::Attr { base, attr } => {
            format!(
                "(attr {} {})",
                ex(base, interner),
                interner.resolve(attr.name)
            )
        }
        ExprKind::Index { base, index } => {
            format!("(index {} {})", ex(base, interner), ex(index, interner))
        }
    }
}

fn ty(t: &TypeExpr, interner: &Interner) -> String {
    match &t.kind {
        TypeExprKind::Name(id) => format!("(ty {})", interner.resolve(id.name)),
        TypeExprKind::Generic { base, args } => {
            let mut parts = vec![interner.resolve(base.name).to_string()];
            for a in args {
                parts.push(ty(a, interner));
            }
            format!("(ty {})", parts.join(" "))
        }
    }
}

fn target(t: &Target, interner: &Interner) -> String {
    match t {
        Target::Name(id) => format!("(name {})", interner.resolve(id.name)),
        Target::Index { base, index } => {
            format!("(index {} {})", ex(base, interner), ex(index, interner))
        }
    }
}

fn body(stmts: &[Stmt], interner: &Interner) -> String {
    stmts
        .iter()
        .map(|s| st(s, interner))
        .collect::<Vec<_>>()
        .join(" ")
}

// Constrói a cadeia if/elif/else num único passo para a frente: cada elif é
// escrito exatamente uma vez (nunca reencaixado num `format!` maior), o que
// evita custo quadrático em cadeias com muitos `elif` (ex.: 100 000).
fn format_if(
    cond: &Expr,
    then: &cudra_ast::Block,
    elifs: &[(Expr, cudra_ast::Block)],
    orelse: &Option<cudra_ast::Block>,
    interner: &Interner,
) -> String {
    let mut out = String::new();
    let pairs = std::iter::once((cond, then)).chain(elifs.iter().map(|(c, b)| (c, b)));
    let n = 1 + elifs.len();
    for (i, (c, blk)) in pairs.enumerate() {
        out.push_str("(if ");
        out.push_str(&ex(c, interner));
        out.push_str(" (body ");
        out.push_str(&body(&blk.stmts, interner));
        out.push(')');
        let is_last = i + 1 == n;
        if !is_last {
            out.push_str(" (else ");
        } else if let Some(o) = orelse {
            out.push_str(" (else ");
            out.push_str(&body(&o.stmts, interner));
            out.push(')');
        }
    }
    for _ in 0..(2 * n - 1) {
        out.push(')');
    }
    out
}

fn st(s: &Stmt, interner: &Interner) -> String {
    match &s.kind {
        StmtKind::Assign {
            target: t,
            ty: ty_opt,
            value,
        } => {
            let ty_str = ty_opt
                .as_ref()
                .map(|t| ty(t, interner))
                .unwrap_or_else(|| "_".to_string());
            format!(
                "(assign {} {} {})",
                target(t, interner),
                ty_str,
                ex(value, interner)
            )
        }
        StmtKind::AugAssign {
            target: t,
            op,
            value,
        } => {
            format!(
                "(aug {} {} {})",
                bin_op_str(*op),
                target(t, interner),
                ex(value, interner)
            )
        }
        StmtKind::For { var, iter, body: b } => {
            format!(
                "(for {} {} (body {}))",
                interner.resolve(var.name),
                ex(iter, interner),
                body(&b.stmts, interner)
            )
        }
        StmtKind::While { cond, body: b } => {
            format!(
                "(while {} (body {}))",
                ex(cond, interner),
                body(&b.stmts, interner)
            )
        }
        StmtKind::If {
            cond,
            then,
            elifs,
            orelse,
        } => format_if(cond, then, elifs, orelse, interner),
        StmtKind::Return(opt) => match opt {
            Some(e) => format!("(return {})", ex(e, interner)),
            None => "(return _)".to_string(),
        },
        StmtKind::Break => "(break)".to_string(),
        StmtKind::Continue => "(continue)".to_string(),
        StmtKind::Pass => "(pass)".to_string(),
        StmtKind::Expr(e) => format!("(expr {})", ex(e, interner)),
        StmtKind::Import { module } => format!("(import {})", interner.resolve(module.name)),
        StmtKind::FromImport { module, names } => {
            let names_str: Vec<_> = names
                .iter()
                .map(|n| interner.resolve(n.name).to_string())
                .collect();
            format!(
                "(from {} {})",
                interner.resolve(module.name),
                names_str.join(" ")
            )
        }
    }
}

fn func_sexpr(f: &FuncDef, interner: &Interner) -> String {
    let decos: Vec<_> = f.decorators.iter().map(|d| ex(d, interner)).collect();
    let params: Vec<_> = f
        .params
        .iter()
        .map(|p| {
            let ty_str =
                p.ty.as_ref()
                    .map(|t| ty(t, interner))
                    .unwrap_or_else(|| "_".to_string());
            format!("(p {} {})", interner.resolve(p.name.name), ty_str)
        })
        .collect();
    let ret_str = f
        .ret
        .as_ref()
        .map(|t| ty(t, interner))
        .unwrap_or_else(|| "_".to_string());
    let body_str = body(&f.body.stmts, interner);
    format!(
        "(def {} {} {} {} (body {}))",
        interner.resolve(f.name.name),
        wrap("decos", &decos.join(" ")),
        wrap("params", &params.join(" ")),
        ret_str,
        body_str
    )
}

fn item_sexpr(it: &Item, interner: &Interner) -> String {
    match it {
        Item::Stmt(s) => st(s, interner),
        Item::Func(f) => func_sexpr(f, interner),
    }
}
