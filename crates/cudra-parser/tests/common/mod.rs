// Reproduz o formato do binário `dump_ast`, usado como oráculo do CPython,
// para os testes de snapshot poderem usar o mesmo formato.
#![allow(dead_code)]

use cudra_ast::{
    BinOp, Block, CmpOp, Expr, ExprKind, FuncDef, Item, LogicOp, Stmt, StmtKind, Target, TypeExpr,
    TypeExprKind, UnOp,
};
use cudra_lexer::lex;
use cudra_parser::parse;
use cudra_span::{Interner, SourceMap};

// Corre numa thread com pilha de 256 MiB, tal como o `dump_ast` e o
// `cudrac`: entradas com aninhamento profundo (ex.: parênteses repetidos)
// dependem dessa pilha maior, não da pilha por omissão da thread de testes.
fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(f)
        .expect("falha ao criar thread de pilha grande")
        .join()
        .expect("thread de pilha grande entrou em pânico")
}

pub fn dump(src: &str) -> String {
    let src = src.to_string();
    on_big_stack(move || {
        let mut sources = SourceMap::new();
        let file = sources.add("t.cdr".to_string(), src.clone());
        let (tokens, lex_diags) = lex(file, &src);

        let mut interner = Interner::new();
        let (module, parse_diags) = parse(file, &src, &tokens, &mut interner);

        let diags = if !lex_diags.is_empty() {
            &lex_diags
        } else {
            &parse_diags
        };

        let mut out = String::new();
        if diags.is_empty() {
            for it in &module.items {
                out.push_str(&item_sexpr(it, &interner));
                out.push('\n');
            }
        }
        for diag in diags {
            let (line, col) = diag
                .labels
                .first()
                .map(|l| sources.line_col(file, l.span.lo))
                .unwrap_or((1, 1));
            out.push_str(&format!("ERROR\t{}\t{line}:{col}\n", diag.message));
        }
        out
    })
}

// larga uma árvore devolvida por `parse_src` na mesma pilha grande em que
// foi construída: cadeias associativas à esquerda com dezenas de milhares
// de nós (ex.: `and`/`+` sem limite de profundidade, ver ronda 2 da parte
// D) recursam na Drop tal como na construção, e a pilha da própria thread
// de testes é pequena demais para isso.
pub fn drop_on_big_stack<T: Send + 'static>(v: T) {
    on_big_stack(move || drop(v));
}

pub fn parse_src(src: &str) -> (cudra_ast::Module, Vec<cudra_diagnostics::Diagnostic>) {
    let src = src.to_string();
    on_big_stack(move || {
        let mut sources = SourceMap::new();
        let file = sources.add("t.cdr".to_string(), src.clone());
        let (tokens, mut lex_diags) = lex(file, &src);
        let mut interner = Interner::new();
        let (module, parse_diags) = parse(file, &src, &tokens, &mut interner);
        lex_diags.extend(parse_diags);
        (module, lex_diags)
    })
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

// Tarefa de impressão pendente: texto já formatado, um nome já resolvido ou
// uma subárvore ainda por expandir.
enum Job<'a, 'b> {
    Lit(&'static str),
    Name(&'b str),
    Node(&'a Expr),
}

// Percorre a expressão com uma pilha explícita, em vez de recursão: cadeias
// associativas muito profundas (ex.: `a+a+…` com centenas de milhares de
// termos) esgotariam a pilha, e o `format!` aninhado reescrevia a cadeia
// acumulada a cada nível, com custo quadrático. A ordem de emissão mantém-se
// idêntica, byte a byte (igual ao `dump_ast`).
fn ex(e: &Expr, interner: &Interner) -> String {
    let mut out = String::new();
    let mut stack = vec![Job::Node(e)];
    while let Some(job) = stack.pop() {
        let node = match job {
            Job::Lit(text) => {
                out.push_str(text);
                continue;
            }
            Job::Name(name) => {
                out.push_str(name);
                continue;
            }
            Job::Node(n) => n,
        };
        match &node.kind {
            ExprKind::Int(v) => out.push_str(&format!("(int {v})")),
            ExprKind::Float(v) => out.push_str(&format!("(float {})", float_hex(*v))),
            ExprKind::Bool(b) => out.push_str(if *b { "(bool True)" } else { "(bool False)" }),
            ExprKind::NoneLit => out.push_str("(none)"),
            ExprKind::Name(id) => {
                out.push_str("(name ");
                out.push_str(interner.resolve(id.name));
                out.push(')');
            }
            ExprKind::Unary { op, operand } => {
                stack.push(Job::Lit(")"));
                stack.push(Job::Node(operand));
                stack.push(Job::Lit(" "));
                stack.push(Job::Lit(un_op_str(*op)));
                stack.push(Job::Lit("(unary "));
            }
            ExprKind::Binary { op, lhs, rhs } => {
                stack.push(Job::Lit(")"));
                stack.push(Job::Node(rhs));
                stack.push(Job::Lit(" "));
                stack.push(Job::Node(lhs));
                stack.push(Job::Lit(" "));
                stack.push(Job::Lit(bin_op_str(*op)));
                stack.push(Job::Lit("(bin "));
            }
            ExprKind::Logical { op, lhs, rhs } => {
                // Achata iterativamente a subárvore da mesma operação (mais
                // à esquerda primeiro), para que `a and b and c` saia como
                // `(and a b c)`.
                let mut parts: Vec<&Expr> = Vec::new();
                let mut pending: Vec<&Expr> = vec![rhs.as_ref(), lhs.as_ref()];
                while let Some(p) = pending.pop() {
                    if let ExprKind::Logical { op: op2, lhs, rhs } = &p.kind {
                        if op2 == op {
                            pending.push(rhs);
                            pending.push(lhs);
                            continue;
                        }
                    }
                    parts.push(p);
                }
                stack.push(Job::Lit(")"));
                for p in parts.iter().rev() {
                    stack.push(Job::Node(p));
                    stack.push(Job::Lit(" "));
                }
                stack.push(Job::Lit(match op {
                    LogicOp::And => "(and",
                    LogicOp::Or => "(or",
                }));
            }
            ExprKind::Compare { first, rest } => {
                stack.push(Job::Lit(")"));
                for (op, c) in rest.iter().rev() {
                    stack.push(Job::Node(c));
                    stack.push(Job::Lit(" "));
                    stack.push(Job::Lit(cmp_op_str(*op)));
                    stack.push(Job::Lit(" "));
                }
                stack.push(Job::Node(first));
                stack.push(Job::Lit("(cmp "));
            }
            ExprKind::Call { callee, args } => {
                stack.push(Job::Lit(")"));
                for a in args.iter().rev() {
                    stack.push(Job::Node(a));
                    stack.push(Job::Lit(" "));
                }
                stack.push(Job::Node(callee));
                stack.push(Job::Lit("(call "));
            }
            ExprKind::Attr { base, attr } => {
                stack.push(Job::Lit(")"));
                stack.push(Job::Name(interner.resolve(attr.name)));
                stack.push(Job::Lit(" "));
                stack.push(Job::Node(base));
                stack.push(Job::Lit("(attr "));
            }
            ExprKind::Index { base, index } => {
                stack.push(Job::Lit(")"));
                stack.push(Job::Node(index));
                stack.push(Job::Lit(" "));
                stack.push(Job::Node(base));
                stack.push(Job::Lit("(index "));
            }
        }
    }
    out
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

fn format_if(
    cond: &Expr,
    then: &Block,
    elifs: &[(Expr, Block)],
    orelse: &Option<Block>,
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
