use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;

use clap::{Parser as ClapParser, Subcommand, ValueEnum};

use cudra_ast::{
    Block, Expr, ExprKind, Item, Module, Stmt, StmtKind, Target, TypeExpr, TypeExprKind,
};
use cudra_diagnostics::render_all;
use cudra_lexer::{lex, TokenKind};
use cudra_parser::parse as parse_module;
use cudra_span::{FileId, Interner, SourceMap};

#[derive(ClapParser)]
#[command(name = "cudrac")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Build {
        src: PathBuf,
        #[arg(short = 'o')]
        out: Option<PathBuf>,
        #[arg(long)]
        release: bool,
        #[arg(long)]
        emit: Option<Emit>,
    },
    Run {
        src: PathBuf,
        #[arg(long)]
        release: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    Check {
        src: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Emit {
    Tokens,
    Ast,
    Tir,
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "erro desconhecido".to_string()
        };
        eprintln!("erro interno do compilador: {msg}");
    }));
}

fn main() -> ExitCode {
    install_panic_hook();
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(2);
        }
    };
    // cadeias de `and`/`or`/operadores binários já não contam para o limite
    // estrutural de aninhamento do parser (podem ter dezenas de milhares de
    // termos), mas continuam a ser árvores genuinamente aninhadas que a
    // sema/TIR percorrem recursivamente — por isso a pilha tem de ser grande.
    let builder = thread::Builder::new().stack_size(1024 << 20);
    let handle = match builder.spawn(move || run(cli)) {
        Ok(h) => h,
        Err(_) => return ExitCode::from(2),
    };
    handle.join().unwrap_or(ExitCode::from(2))
}

fn run(cli: Cli) -> ExitCode {
    if std::env::var("CUDRAC_TEST_PANIC").is_ok() {
        panic!("panic de teste (CUDRAC_TEST_PANIC)");
    }
    match cli.command {
        Command::Check { src } => cmd_check(&src),
        Command::Build {
            src,
            emit: Some(emit),
            ..
        } => cmd_emit(&src, emit),
        Command::Build { .. } | Command::Run { .. } => {
            eprintln!("erro: ainda não implementado na Fase 1");
            ExitCode::from(2)
        }
    }
}

fn read_source(path: &PathBuf) -> Result<String, ExitCode> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("erro: não foi possível ler '{}': {e}", path.display());
            return Err(ExitCode::from(2));
        }
    };
    match String::from_utf8(bytes) {
        Ok(s) => Ok(s),
        Err(e) => {
            eprintln!("erro: '{}' não é UTF-8 válido: {e}", path.display());
            Err(ExitCode::from(1))
        }
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

fn render_diags(diags: &[cudra_diagnostics::Diagnostic], sources: &SourceMap) {
    let color = io::stderr().is_terminal();
    eprint!("{}", render_all(diags, sources, color));
}

fn cmd_check(src_path: &PathBuf) -> ExitCode {
    let src = match read_source(src_path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let mut sources = SourceMap::new();
    let file = sources.add(src_path.display().to_string(), src.clone());
    let (tokens, lex_diags) = lex(file, &src);
    let mut interner = Interner::new();
    let (module, parse_diags) = parse_module(file, &src, &tokens, &mut interner);
    let diags = if lex_diags.is_empty() {
        parse_diags
    } else {
        lex_diags
    };
    if !diags.is_empty() {
        render_diags(&diags, &sources);
        return ExitCode::from(1);
    }
    match cudra_sema::check(&module, &interner) {
        Ok(tir_module) => {
            if let Err(e) = cudra_tir::verify(&tir_module) {
                eprintln!("erro interno do compilador: {e}");
                return ExitCode::from(2);
            }
            ExitCode::SUCCESS
        }
        Err(sema_diags) => {
            render_diags(&sema_diags, &sources);
            ExitCode::from(1)
        }
    }
}

fn cmd_emit(src_path: &PathBuf, emit: Emit) -> ExitCode {
    let src = match read_source(src_path) {
        Ok(s) => s,
        Err(c) => return c,
    };
    let mut sources = SourceMap::new();
    let file = sources.add(src_path.display().to_string(), src.clone());
    let (tokens, lex_diags) = lex(file, &src);

    match emit {
        Emit::Tokens => {
            let stdout = io::stdout();
            let mut out = io::BufWriter::new(stdout.lock());
            for tok in &tokens {
                let text = match tok.kind {
                    TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof => String::new(),
                    _ => escape(&src[tok.span.lo as usize..tok.span.hi as usize]),
                };
                let (line, col) = sources.line_col(file, tok.span.lo);
                if writeln!(out, "{:?}\t{text}\t{line}:{col}", tok.kind).is_err() {
                    break;
                }
            }
            let _ = out.flush();
            if lex_diags.is_empty() {
                ExitCode::SUCCESS
            } else {
                render_diags(&lex_diags, &sources);
                ExitCode::from(1)
            }
        }
        Emit::Ast => {
            let mut interner = Interner::new();
            let (module, parse_diags) = parse_module(file, &src, &tokens, &mut interner);
            let diags = if lex_diags.is_empty() {
                parse_diags
            } else {
                lex_diags
            };
            if diags.is_empty() {
                let stdout = io::stdout();
                let mut out = io::BufWriter::new(stdout.lock());
                let _ = print_module(&mut out, &module, &sources, file, &interner);
                let _ = out.flush();
            }
            render_diags(&diags, &sources);
            if diags.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Emit::Tir => {
            let mut interner = Interner::new();
            let (module, parse_diags) = parse_module(file, &src, &tokens, &mut interner);
            let diags = if lex_diags.is_empty() {
                parse_diags
            } else {
                lex_diags
            };
            if !diags.is_empty() {
                render_diags(&diags, &sources);
                return ExitCode::from(1);
            }
            match cudra_sema::check(&module, &interner) {
                Ok(tir_module) => {
                    if let Err(e) = cudra_tir::verify(&tir_module) {
                        eprintln!("erro interno do compilador: {e}");
                        return ExitCode::from(2);
                    }
                    print!("{}", cudra_tir::print_module(&tir_module));
                    ExitCode::SUCCESS
                }
                Err(sema_diags) => {
                    render_diags(&sema_diags, &sources);
                    ExitCode::from(1)
                }
            }
        }
    }
}

// --- `--emit=ast`: árvore legível, 2 espaços por nível, linha:col em cada nó.

fn lc(sm: &SourceMap, file: FileId, span: cudra_span::Span) -> (u32, u32) {
    sm.line_col(file, span.lo)
}

fn iw(out: &mut impl Write, depth: usize) -> io::Result<()> {
    for _ in 0..depth {
        write!(out, "  ")?;
    }
    Ok(())
}

fn print_module(
    out: &mut impl Write,
    m: &Module,
    sm: &SourceMap,
    file: FileId,
    interner: &Interner,
) -> io::Result<()> {
    for item in &m.items {
        print_item(out, item, 0, sm, file, interner)?;
    }
    Ok(())
}

fn print_item(
    out: &mut impl Write,
    item: &Item,
    depth: usize,
    sm: &SourceMap,
    file: FileId,
    interner: &Interner,
) -> io::Result<()> {
    match item {
        Item::Func(f) => {
            let (l, c) = lc(sm, file, f.span);
            iw(out, depth)?;
            writeln!(out, "Func {} {l}:{c}", interner.resolve(f.name.name))?;
            for d in &f.decorators {
                iw(out, depth + 1)?;
                writeln!(out, "Decorator")?;
                print_expr(out, d, depth + 2, sm, file, interner)?;
            }
            for p in &f.params {
                let (pl, pc) = lc(sm, file, p.span);
                iw(out, depth + 1)?;
                writeln!(out, "Param {} {pl}:{pc}", interner.resolve(p.name.name))?;
                if let Some(t) = &p.ty {
                    print_type(out, t, depth + 2, sm, file, interner)?;
                }
            }
            if let Some(r) = &f.ret {
                iw(out, depth + 1)?;
                writeln!(out, "Ret")?;
                print_type(out, r, depth + 2, sm, file, interner)?;
            }
            iw(out, depth + 1)?;
            writeln!(out, "Body")?;
            for s in &f.body.stmts {
                print_stmt(out, s, depth + 2, sm, file, interner)?;
            }
            Ok(())
        }
        Item::Stmt(s) => print_stmt(out, s, depth, sm, file, interner),
    }
}

fn print_block(
    out: &mut impl Write,
    b: &Block,
    depth: usize,
    sm: &SourceMap,
    file: FileId,
    interner: &Interner,
) -> io::Result<()> {
    for s in &b.stmts {
        print_stmt(out, s, depth, sm, file, interner)?;
    }
    Ok(())
}

fn print_target(
    out: &mut impl Write,
    t: &Target,
    depth: usize,
    sm: &SourceMap,
    file: FileId,
    interner: &Interner,
) -> io::Result<()> {
    match t {
        Target::Name(id) => {
            let (l, c) = lc(sm, file, id.span);
            iw(out, depth)?;
            writeln!(out, "Target.Name {} {l}:{c}", interner.resolve(id.name))
        }
        Target::Index { base, index } => {
            iw(out, depth)?;
            writeln!(out, "Target.Index")?;
            print_expr(out, base, depth + 1, sm, file, interner)?;
            print_expr(out, index, depth + 1, sm, file, interner)
        }
    }
}

fn print_stmt(
    out: &mut impl Write,
    s: &Stmt,
    depth: usize,
    sm: &SourceMap,
    file: FileId,
    interner: &Interner,
) -> io::Result<()> {
    let (l, c) = lc(sm, file, s.span);
    match &s.kind {
        StmtKind::Assign { target, ty, value } => {
            iw(out, depth)?;
            writeln!(out, "Assign {l}:{c}")?;
            print_target(out, target, depth + 1, sm, file, interner)?;
            if let Some(t) = ty {
                print_type(out, t, depth + 1, sm, file, interner)?;
            }
            print_expr(out, value, depth + 1, sm, file, interner)
        }
        StmtKind::AugAssign { target, op, value } => {
            iw(out, depth)?;
            writeln!(out, "AugAssign {op:?} {l}:{c}")?;
            print_target(out, target, depth + 1, sm, file, interner)?;
            print_expr(out, value, depth + 1, sm, file, interner)
        }
        StmtKind::For { var, iter, body } => {
            iw(out, depth)?;
            writeln!(out, "For {} {l}:{c}", interner.resolve(var.name))?;
            print_expr(out, iter, depth + 1, sm, file, interner)?;
            print_block(out, body, depth + 1, sm, file, interner)
        }
        StmtKind::While { cond, body } => {
            iw(out, depth)?;
            writeln!(out, "While {l}:{c}")?;
            print_expr(out, cond, depth + 1, sm, file, interner)?;
            print_block(out, body, depth + 1, sm, file, interner)
        }
        StmtKind::If {
            cond,
            then,
            elifs,
            orelse,
        } => {
            iw(out, depth)?;
            writeln!(out, "If {l}:{c}")?;
            print_expr(out, cond, depth + 1, sm, file, interner)?;
            print_block(out, then, depth + 1, sm, file, interner)?;
            for (c2, b2) in elifs {
                iw(out, depth)?;
                writeln!(out, "Elif")?;
                print_expr(out, c2, depth + 1, sm, file, interner)?;
                print_block(out, b2, depth + 1, sm, file, interner)?;
            }
            if let Some(o) = orelse {
                iw(out, depth)?;
                writeln!(out, "Else")?;
                print_block(out, o, depth + 1, sm, file, interner)?;
            }
            Ok(())
        }
        StmtKind::Return(opt) => {
            iw(out, depth)?;
            writeln!(out, "Return {l}:{c}")?;
            if let Some(e) = opt {
                print_expr(out, e, depth + 1, sm, file, interner)?;
            }
            Ok(())
        }
        StmtKind::Break => {
            iw(out, depth)?;
            writeln!(out, "Break {l}:{c}")
        }
        StmtKind::Continue => {
            iw(out, depth)?;
            writeln!(out, "Continue {l}:{c}")
        }
        StmtKind::Pass => {
            iw(out, depth)?;
            writeln!(out, "Pass {l}:{c}")
        }
        StmtKind::Expr(e) => {
            iw(out, depth)?;
            writeln!(out, "ExprStmt {l}:{c}")?;
            print_expr(out, e, depth + 1, sm, file, interner)
        }
        StmtKind::Import { module } => {
            iw(out, depth)?;
            writeln!(out, "Import {} {l}:{c}", interner.resolve(module.name))
        }
        StmtKind::FromImport { module, names } => {
            iw(out, depth)?;
            let ns: Vec<_> = names.iter().map(|n| interner.resolve(n.name)).collect();
            writeln!(
                out,
                "FromImport {} [{}] {l}:{c}",
                interner.resolve(module.name),
                ns.join(", ")
            )
        }
    }
}

fn print_type(
    out: &mut impl Write,
    t: &TypeExpr,
    depth: usize,
    sm: &SourceMap,
    file: FileId,
    interner: &Interner,
) -> io::Result<()> {
    let (l, c) = lc(sm, file, t.span);
    match &t.kind {
        TypeExprKind::Name(id) => {
            iw(out, depth)?;
            writeln!(out, "Type {} {l}:{c}", interner.resolve(id.name))
        }
        TypeExprKind::Generic { base, args } => {
            iw(out, depth)?;
            writeln!(out, "TypeGeneric {} {l}:{c}", interner.resolve(base.name))?;
            for a in args {
                print_type(out, a, depth + 1, sm, file, interner)?;
            }
            Ok(())
        }
    }
}

fn print_expr(
    out: &mut impl Write,
    e: &Expr,
    depth: usize,
    sm: &SourceMap,
    file: FileId,
    interner: &Interner,
) -> io::Result<()> {
    let (l, c) = lc(sm, file, e.span);
    match &e.kind {
        ExprKind::Int(v) => {
            iw(out, depth)?;
            writeln!(out, "Int {v} {l}:{c}")
        }
        ExprKind::Float(v) => {
            iw(out, depth)?;
            writeln!(out, "Float {v} {l}:{c}")
        }
        ExprKind::Bool(b) => {
            iw(out, depth)?;
            writeln!(out, "Bool {b} {l}:{c}")
        }
        ExprKind::NoneLit => {
            iw(out, depth)?;
            writeln!(out, "None {l}:{c}")
        }
        ExprKind::Name(id) => {
            iw(out, depth)?;
            writeln!(out, "Name {} {l}:{c}", interner.resolve(id.name))
        }
        ExprKind::Unary { op, operand } => {
            iw(out, depth)?;
            writeln!(out, "Unary {op:?} {l}:{c}")?;
            print_expr(out, operand, depth + 1, sm, file, interner)
        }
        ExprKind::Binary { op, lhs, rhs } => {
            iw(out, depth)?;
            writeln!(out, "Binary {op:?} {l}:{c}")?;
            print_expr(out, lhs, depth + 1, sm, file, interner)?;
            print_expr(out, rhs, depth + 1, sm, file, interner)
        }
        ExprKind::Logical { op, lhs, rhs } => {
            iw(out, depth)?;
            writeln!(out, "Logical {op:?} {l}:{c}")?;
            print_expr(out, lhs, depth + 1, sm, file, interner)?;
            print_expr(out, rhs, depth + 1, sm, file, interner)
        }
        ExprKind::Compare { first, rest } => {
            iw(out, depth)?;
            writeln!(out, "Compare {l}:{c}")?;
            print_expr(out, first, depth + 1, sm, file, interner)?;
            for (op, c2) in rest {
                iw(out, depth + 1)?;
                writeln!(out, "Op {op:?}")?;
                print_expr(out, c2, depth + 2, sm, file, interner)?;
            }
            Ok(())
        }
        ExprKind::Call { callee, args } => {
            iw(out, depth)?;
            writeln!(out, "Call {l}:{c}")?;
            print_expr(out, callee, depth + 1, sm, file, interner)?;
            for a in args {
                print_expr(out, a, depth + 1, sm, file, interner)?;
            }
            Ok(())
        }
        ExprKind::Attr { base, attr } => {
            iw(out, depth)?;
            writeln!(out, "Attr {} {l}:{c}", interner.resolve(attr.name))?;
            print_expr(out, base, depth + 1, sm, file, interner)
        }
        ExprKind::Index { base, index } => {
            iw(out, depth)?;
            writeln!(out, "Index {l}:{c}")?;
            print_expr(out, base, depth + 1, sm, file, interner)?;
            print_expr(out, index, depth + 1, sm, file, interner)
        }
    }
}
