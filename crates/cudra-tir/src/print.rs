use std::fmt::Write as _;

use crate::{Func, Intrinsic, LocalId, Module, TCmp, TExpr, TExprKind, TStmt, Ty};

/// Texto legível e determinista da TIR (material de demo, formato estável
/// para snapshots): uma função por bloco, locais com nome e tipo,
/// instruções indentadas, expressões em forma prefixa com o tipo.
pub fn print_module(m: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "main: f{}", m.main.0);
    for f in &m.funcs {
        print_func(&mut out, f);
    }
    out
}

fn ty_name(t: Ty) -> &'static str {
    match t {
        Ty::Int => "int",
        Ty::Float => "float",
        Ty::Bool => "bool",
        Ty::NoneTy => "None",
    }
}

fn local_ref(f: &Func, id: LocalId) -> String {
    format!("{}${}", f.locals[id.0 as usize].name, id.0)
}

fn print_func(out: &mut String, f: &Func) {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            format!(
                "{}: {}",
                local_ref(f, *p),
                ty_name(f.locals[p.0 as usize].ty)
            )
        })
        .collect();
    let _ = writeln!(
        out,
        "func f{} {}({}) -> {}",
        f.id.0,
        f.name,
        params.join(", "),
        ty_name(f.ret)
    );
    let _ = writeln!(out, "  locals:");
    for (i, l) in f.locals.iter().enumerate() {
        let _ = writeln!(out, "    l{i} {}: {}", l.name, ty_name(l.ty));
    }
    let _ = writeln!(out, "  body:");
    print_stmts(out, f, &f.body, 2);
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn print_stmts(out: &mut String, f: &Func, stmts: &[TStmt], depth: usize) {
    for s in stmts {
        print_stmt(out, f, s, depth);
    }
}

fn print_stmt(out: &mut String, f: &Func, s: &TStmt, depth: usize) {
    indent(out, depth);
    match s {
        TStmt::Store { local, value } => {
            let _ = write!(out, "Store {} = ", local_ref(f, *local));
            print_expr(out, f, value);
            out.push('\n');
        }
        TStmt::ForRange {
            var,
            start,
            stop,
            step,
            body,
        } => {
            let _ = write!(out, "ForRange {} = ", local_ref(f, *var));
            print_expr(out, f, start);
            out.push_str(" .. ");
            print_expr(out, f, stop);
            out.push_str(" step ");
            print_expr(out, f, step);
            out.push('\n');
            print_stmts(out, f, body, depth + 1);
        }
        TStmt::While { cond, body } => {
            out.push_str("While ");
            print_expr(out, f, cond);
            out.push('\n');
            print_stmts(out, f, body, depth + 1);
        }
        TStmt::If { cond, then, orelse } => {
            out.push_str("If ");
            print_expr(out, f, cond);
            out.push('\n');
            print_stmts(out, f, then, depth + 1);
            if !orelse.is_empty() {
                indent(out, depth);
                out.push_str("Else\n");
                print_stmts(out, f, orelse, depth + 1);
            }
        }
        TStmt::Return(opt) => {
            out.push_str("Return");
            if let Some(e) = opt {
                out.push(' ');
                print_expr(out, f, e);
            }
            out.push('\n');
        }
        TStmt::Break => out.push_str("Break\n"),
        TStmt::Continue => out.push_str("Continue\n"),
        TStmt::Expr(e) => {
            out.push_str("Expr ");
            print_expr(out, f, e);
            out.push('\n');
        }
    }
}

fn print_expr(out: &mut String, f: &Func, e: &TExpr) {
    match &e.kind {
        TExprKind::ConstInt(v) => {
            let _ = write!(out, "{v}:int");
        }
        TExprKind::ConstFloat(v) => {
            let _ = write!(out, "{v:?}:float");
        }
        TExprKind::ConstBool(v) => {
            let _ = write!(out, "{v}:bool");
        }
        TExprKind::ConstNone => out.push_str("None"),
        TExprKind::Local(id) => {
            let _ = write!(out, "{}:{}", local_ref(f, *id), ty_name(e.ty));
        }
        TExprKind::Binary { op, lhs, rhs } => {
            let _ = write!(out, "({op:?} ");
            print_expr(out, f, lhs);
            out.push(' ');
            print_expr(out, f, rhs);
            let _ = write!(out, "):{}", ty_name(e.ty));
        }
        TExprKind::Unary { op, operand } => {
            let _ = write!(out, "({op:?} ");
            print_expr(out, f, operand);
            let _ = write!(out, "):{}", ty_name(e.ty));
        }
        TExprKind::Compare { first, rest } => {
            out.push_str("(Compare ");
            print_expr(out, f, first);
            for (cmp, rhs) in rest {
                let _ = write!(out, " {} ", cmp_name(*cmp));
                print_expr(out, f, rhs);
            }
            out.push_str("):bool");
        }
        TExprKind::And(a, b) => {
            out.push_str("(And ");
            print_expr(out, f, a);
            out.push(' ');
            print_expr(out, f, b);
            out.push_str("):bool");
        }
        TExprKind::Or(a, b) => {
            out.push_str("(Or ");
            print_expr(out, f, a);
            out.push(' ');
            print_expr(out, f, b);
            out.push_str("):bool");
        }
        TExprKind::Cast { value } => {
            out.push_str("(Cast ");
            print_expr(out, f, value);
            let _ = write!(out, "):{}", ty_name(e.ty));
        }
        TExprKind::Call { func, args } => {
            let _ = write!(out, "(Call f{} ", func.0);
            print_args(out, f, args);
            let _ = write!(out, "):{}", ty_name(e.ty));
        }
        TExprKind::Intrinsic { which, args } => {
            let _ = write!(out, "(Intrinsic {} ", intrinsic_name(*which));
            print_args(out, f, args);
            let _ = write!(out, "):{}", ty_name(e.ty));
        }
    }
}

fn print_args(out: &mut String, f: &Func, args: &[TExpr]) {
    out.push('[');
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        print_expr(out, f, a);
    }
    out.push(']');
}

fn cmp_name(cmp: TCmp) -> String {
    let (kind, op) = match cmp {
        TCmp::I(op) => ("I", op),
        TCmp::F(op) => ("F", op),
        TCmp::B(op) => ("B", op),
        TCmp::IntFloat(op) => ("IntFloat", op),
        TCmp::FloatInt(op) => ("FloatInt", op),
    };
    format!("{kind}.{op:?}")
}

fn intrinsic_name(i: Intrinsic) -> &'static str {
    match i {
        Intrinsic::Print => "Print",
        Intrinsic::AbsInt => "AbsInt",
        Intrinsic::AbsFloat => "AbsFloat",
        Intrinsic::ArgInt => "ArgInt",
        Intrinsic::ArgFloat => "ArgFloat",
    }
}
