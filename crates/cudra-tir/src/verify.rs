use crate::{
    Func, Intrinsic, Local, LocalId, Module, TBinOp, TCmp, TExpr, TExprKind, TStmt, TUnOp, Ty,
};

struct Ctx<'a> {
    funcs: &'a [Func],
    locals: &'a [Local],
}

/// Verificador estrutural completo: tipos dos operandos, `Store` com o tipo
/// do local, aridade/tipos de `Call`, `Return` com o tipo de `ret`,
/// condições `bool`, `Break`/`Continue` só dentro de ciclos, `LocalId`/
/// `FuncId` dentro dos limites, `Cast` só nos pares válidos.
pub fn verify(m: &Module) -> Result<(), String> {
    if m.main.0 as usize >= m.funcs.len() {
        return Err(format!("FuncId de main fora dos limites: {}", m.main.0));
    }
    for (i, f) in m.funcs.iter().enumerate() {
        if f.id.0 as usize != i {
            return Err(format!(
                "Func no índice {i} tem FuncId {} (esperava-se {i})",
                f.id.0
            ));
        }
        for p in &f.params {
            check_local_bounds(f, *p)?;
        }
        let ctx = Ctx {
            funcs: &m.funcs,
            locals: &f.locals,
        };
        verify_block(&ctx, &f.body, f.ret, false)?;
    }
    Ok(())
}

fn check_local_bounds(f: &Func, id: LocalId) -> Result<(), String> {
    if id.0 as usize >= f.locals.len() {
        return Err(format!(
            "LocalId {} fora dos limites em '{}' ({} locais)",
            id.0,
            f.name,
            f.locals.len()
        ));
    }
    Ok(())
}

fn verify_block(ctx: &Ctx, stmts: &[TStmt], ret: Ty, in_loop: bool) -> Result<(), String> {
    for s in stmts {
        verify_stmt(ctx, s, ret, in_loop)?;
    }
    Ok(())
}

fn verify_stmt(ctx: &Ctx, s: &TStmt, ret: Ty, in_loop: bool) -> Result<(), String> {
    match s {
        TStmt::Store { local, value } => {
            check_local_bounds_ctx(ctx, *local)?;
            let vt = verify_expr(ctx, value)?;
            let lt = ctx.locals[local.0 as usize].ty;
            if vt != lt {
                return Err(format!(
                    "Store em '{}': valor {vt:?}, local {lt:?}",
                    ctx.locals[local.0 as usize].name
                ));
            }
            Ok(())
        }
        TStmt::ForRange {
            var,
            start,
            stop,
            step,
            body,
        } => {
            check_local_bounds_ctx(ctx, *var)?;
            let vt = ctx.locals[var.0 as usize].ty;
            if vt != Ty::Int {
                return Err(format!("ForRange: variável não é int ({vt:?})"));
            }
            for (name, e) in [("start", start), ("stop", stop), ("step", step)] {
                let t = verify_expr(ctx, e)?;
                if t != Ty::Int {
                    return Err(format!("ForRange: {name} não é int ({t:?})"));
                }
            }
            verify_block(ctx, body, ret, true)
        }
        TStmt::While { cond, body } => {
            let ct = verify_expr(ctx, cond)?;
            if ct != Ty::Bool {
                return Err(format!("While: condição não é bool ({ct:?})"));
            }
            verify_block(ctx, body, ret, true)
        }
        TStmt::If { cond, then, orelse } => {
            let ct = verify_expr(ctx, cond)?;
            if ct != Ty::Bool {
                return Err(format!("If: condição não é bool ({ct:?})"));
            }
            verify_block(ctx, then, ret, in_loop)?;
            verify_block(ctx, orelse, ret, in_loop)
        }
        TStmt::Return(opt) => match opt {
            None => {
                if ret != Ty::NoneTy {
                    return Err(format!("Return sem valor mas ret é {ret:?}"));
                }
                Ok(())
            }
            Some(e) => {
                let t = verify_expr(ctx, e)?;
                if t != ret {
                    return Err(format!("Return {t:?}, mas a função devolve {ret:?}"));
                }
                Ok(())
            }
        },
        TStmt::Break => {
            if !in_loop {
                return Err("Break fora de um ciclo".to_string());
            }
            Ok(())
        }
        TStmt::Continue => {
            if !in_loop {
                return Err("Continue fora de um ciclo".to_string());
            }
            Ok(())
        }
        TStmt::Expr(e) => verify_expr(ctx, e).map(|_| ()),
    }
}

fn check_local_bounds_ctx(ctx: &Ctx, id: LocalId) -> Result<(), String> {
    if id.0 as usize >= ctx.locals.len() {
        return Err(format!(
            "LocalId {} fora dos limites ({} locais)",
            id.0,
            ctx.locals.len()
        ));
    }
    Ok(())
}

fn verify_expr(ctx: &Ctx, e: &TExpr) -> Result<Ty, String> {
    if let TExprKind::Cast { value } = &e.kind {
        let vt = verify_expr(ctx, value)?;
        if !cast_pair_valid(vt, e.ty) {
            return Err(format!("Cast inválido de {vt:?} para {:?}", e.ty));
        }
        return Ok(e.ty);
    }
    let computed = verify_expr_kind(ctx, &e.kind)?;
    if computed != e.ty {
        return Err(format!(
            "tipo anotado {:?} não bate com o tipo calculado {:?} em {:?}",
            e.ty, computed, e.kind
        ));
    }
    Ok(e.ty)
}

// pares válidos: int<->float, bool->int, bool->float, int->bool, float->bool
// (ver o comentário em `TExprKind::Cast`, em lib.rs).
fn cast_pair_valid(from: Ty, to: Ty) -> bool {
    matches!(
        (from, to),
        (Ty::Int, Ty::Float)
            | (Ty::Float, Ty::Int)
            | (Ty::Bool, Ty::Int)
            | (Ty::Bool, Ty::Float)
            | (Ty::Int, Ty::Bool)
            | (Ty::Float, Ty::Bool)
    )
}

fn verify_expr_kind(ctx: &Ctx, kind: &TExprKind) -> Result<Ty, String> {
    match kind {
        TExprKind::ConstInt(_) => Ok(Ty::Int),
        TExprKind::ConstFloat(_) => Ok(Ty::Float),
        TExprKind::ConstBool(_) => Ok(Ty::Bool),
        TExprKind::ConstNone => Ok(Ty::NoneTy),
        TExprKind::Local(id) => {
            check_local_bounds_ctx(ctx, *id)?;
            Ok(ctx.locals[id.0 as usize].ty)
        }
        TExprKind::Binary { op, lhs, rhs } => {
            let lt = verify_expr(ctx, lhs)?;
            let rt = verify_expr(ctx, rhs)?;
            binop_result(*op, lt, rt)
        }
        TExprKind::Unary { op, operand } => {
            let ot = verify_expr(ctx, operand)?;
            unop_result(*op, ot)
        }
        TExprKind::Compare { first, rest } => {
            let mut prev = verify_expr(ctx, first)?;
            if rest.is_empty() {
                return Err("Compare sem nenhuma comparação".to_string());
            }
            for (cmp, rhs) in rest {
                let rt = verify_expr(ctx, rhs)?;
                check_cmp(*cmp, prev, rt)?;
                prev = rt;
            }
            Ok(Ty::Bool)
        }
        TExprKind::And(a, b) | TExprKind::Or(a, b) => {
            let at = verify_expr(ctx, a)?;
            let bt = verify_expr(ctx, b)?;
            if at != Ty::Bool || bt != Ty::Bool {
                return Err(format!("and/or com operandos não-bool ({at:?}, {bt:?})"));
            }
            Ok(Ty::Bool)
        }
        TExprKind::Cast { .. } => unreachable!("Cast é tratado em verify_expr"),
        TExprKind::Call { func, args } => {
            if func.0 as usize >= ctx.funcs.len() {
                return Err(format!("FuncId {} fora dos limites", func.0));
            }
            let callee = &ctx.funcs[func.0 as usize];
            if args.len() != callee.params.len() {
                return Err(format!(
                    "chamada a '{}' com {} argumentos, esperavam-se {}",
                    callee.name,
                    args.len(),
                    callee.params.len()
                ));
            }
            for (a, p) in args.iter().zip(&callee.params) {
                let at = verify_expr(ctx, a)?;
                let pt = callee.locals[p.0 as usize].ty;
                if at != pt {
                    return Err(format!(
                        "chamada a '{}': argumento {at:?}, parâmetro {pt:?}",
                        callee.name
                    ));
                }
            }
            Ok(callee.ret)
        }
        TExprKind::Intrinsic { which, args } => verify_intrinsic(ctx, *which, args),
    }
}

fn binop_result(op: TBinOp, lt: Ty, rt: Ty) -> Result<Ty, String> {
    use TBinOp::*;
    let (want, result) = match op {
        IAdd | ISub | IMul | IFloorDiv | IMod | IPow | IAnd | IOr | IXor | IShl | IShr => {
            (Ty::Int, Ty::Int)
        }
        ITrueDiv => (Ty::Int, Ty::Float),
        FAdd | FSub | FMul | FDiv | FFloorDiv | FMod | FPow => (Ty::Float, Ty::Float),
    };
    if lt != want || rt != want {
        return Err(format!(
            "{op:?}: operandos {lt:?}/{rt:?}, esperava-se {want:?}"
        ));
    }
    Ok(result)
}

fn unop_result(op: TUnOp, ot: Ty) -> Result<Ty, String> {
    use TUnOp::*;
    let want = match op {
        INeg | IInvert => Ty::Int,
        FNeg => Ty::Float,
        Not => Ty::Bool,
    };
    if ot != want {
        return Err(format!("{op:?}: operando {ot:?}, esperava-se {want:?}"));
    }
    Ok(want)
}

fn check_cmp(cmp: TCmp, lt: Ty, rt: Ty) -> Result<(), String> {
    let (wl, wr) = match cmp {
        TCmp::I(_) => (Ty::Int, Ty::Int),
        TCmp::F(_) => (Ty::Float, Ty::Float),
        TCmp::B(_) => (Ty::Bool, Ty::Bool),
        TCmp::IntFloat(_) => (Ty::Int, Ty::Float),
        TCmp::FloatInt(_) => (Ty::Float, Ty::Int),
    };
    if lt != wl || rt != wr {
        return Err(format!(
            "comparação {cmp:?}: operandos {lt:?}/{rt:?}, esperava-se {wl:?}/{wr:?}"
        ));
    }
    Ok(())
}

fn verify_intrinsic(ctx: &Ctx, which: Intrinsic, args: &[TExpr]) -> Result<Ty, String> {
    match which {
        Intrinsic::Print => {
            for a in args {
                verify_expr(ctx, a)?;
            }
            Ok(Ty::NoneTy)
        }
        Intrinsic::AbsInt => check_unary_intrinsic(ctx, "abs_int", args, Ty::Int, Ty::Int),
        Intrinsic::AbsFloat => check_unary_intrinsic(ctx, "abs_float", args, Ty::Float, Ty::Float),
        Intrinsic::ArgInt => check_unary_intrinsic(ctx, "arg_int", args, Ty::Int, Ty::Int),
        Intrinsic::ArgFloat => check_unary_intrinsic(ctx, "arg_float", args, Ty::Int, Ty::Float),
    }
}

fn check_unary_intrinsic(
    ctx: &Ctx,
    name: &str,
    args: &[TExpr],
    want: Ty,
    result: Ty,
) -> Result<Ty, String> {
    if args.len() != 1 {
        return Err(format!(
            "{name}: esperava-se 1 argumento, há {}",
            args.len()
        ));
    }
    let at = verify_expr(ctx, &args[0])?;
    if at != want {
        return Err(format!("{name}: argumento {at:?}, esperava-se {want:?}"));
    }
    Ok(result)
}
