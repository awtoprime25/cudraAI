use std::collections::{HashMap, HashSet};

use cudra_ast as ast;
use cudra_diagnostics::Diagnostic;
use cudra_span::{FileId, Span, Symbol};
use cudra_tir::{
    CmpOp, Func as TFunc, FuncId, Intrinsic, Local as TLocal, LocalId, Module as TModule, TCmp,
    TExpr, TExprKind, TStmt, Ty,
};

use crate::resolve::{self, ResolvePlan};
use crate::{definite, infer, CheckCtx, STy};

struct LExpr {
    sty: STy,
    t: TExpr,
}

fn error_lexpr(span: Span) -> LExpr {
    LExpr {
        sty: STy::Error,
        t: TExpr {
            ty: Ty::NoneTy,
            kind: TExprKind::ConstNone,
            span,
        },
    }
}

fn cast_to(e: TExpr, ty: Ty) -> TExpr {
    let span = e.span;
    TExpr {
        ty,
        kind: TExprKind::Cast { value: Box::new(e) },
        span,
    }
}

enum LoopKind {
    Normal,
    Infinite,
}

struct LoopCtx {
    kind: LoopKind,
    break_snapshots: Vec<Vec<bool>>,
}

struct FuncChecker {
    is_main: bool,
    cur_func_name: Symbol,
    ret_annotated: bool,
    ret_sty: STy,
    return_sites: Vec<STy>,
    locals: HashMap<Symbol, (LocalId, STy, Span)>,
    locals_hint: HashSet<Symbol>,
    tir_locals: Vec<TLocal>,
    defined: Vec<bool>,
    loop_stack: Vec<LoopCtx>,
    top_pos: Option<usize>,
    block_depth: u32,
    dead: bool,
    name_span: Span,
    ret_annot_span: Option<Span>,
    body_end: Span,
}

/// Spans do `def` usados nos diagnósticos que apontam para a assinatura.
#[derive(Clone, Copy)]
struct FuncSpans {
    name: Span,
    ret_annot: Option<Span>,
    body_end: Span,
}

impl FuncChecker {
    fn new(
        is_main: bool,
        name: Symbol,
        ret_annotated: bool,
        ret_sty: STy,
        locals_hint: HashSet<Symbol>,
        spans: FuncSpans,
    ) -> Self {
        FuncChecker {
            is_main,
            cur_func_name: name,
            ret_annotated,
            ret_sty,
            return_sites: Vec::new(),
            locals: HashMap::new(),
            locals_hint,
            tir_locals: Vec::new(),
            defined: Vec::new(),
            loop_stack: Vec::new(),
            top_pos: None,
            block_depth: 0,
            dead: false,
            name_span: spans.name,
            ret_annot_span: spans.ret_annot,
            body_end: spans.body_end,
        }
    }

    fn new_local(&mut self, ctx: &CheckCtx, name: Symbol, sty: STy) -> LocalId {
        let id = LocalId(self.tir_locals.len() as u32);
        self.tir_locals.push(TLocal {
            name: ctx.sym(name).to_string(),
            ty: sty.to_tir(),
        });
        self.defined.push(false);
        id
    }

    fn truthy_bool(&mut self, ctx: &mut CheckCtx, e: LExpr, span: Span) -> TExpr {
        match e.sty {
            STy::Bool => e.t,
            STy::Int => TExpr {
                ty: Ty::Bool,
                kind: TExprKind::Compare {
                    first: Box::new(e.t),
                    rest: vec![(
                        TCmp::I(CmpOp::Ne),
                        TExpr {
                            ty: Ty::Int,
                            kind: TExprKind::ConstInt(0),
                            span,
                        },
                    )],
                },
                span,
            },
            STy::Float => TExpr {
                ty: Ty::Bool,
                kind: TExprKind::Compare {
                    first: Box::new(e.t),
                    rest: vec![(
                        TCmp::F(CmpOp::Ne),
                        TExpr {
                            ty: Ty::Float,
                            kind: TExprKind::ConstFloat(0.0),
                            span,
                        },
                    )],
                },
                span,
            },
            STy::NoneTy => {
                ctx.err(
                    Diagnostic::error("operações com None ainda não são suportadas")
                        .with_primary(span, "condição None"),
                );
                TExpr {
                    ty: Ty::Bool,
                    kind: TExprKind::ConstBool(false),
                    span,
                }
            }
            STy::Error => TExpr {
                ty: Ty::Bool,
                kind: TExprKind::ConstBool(false),
                span,
            },
        }
    }

    fn apply_binop(
        &mut self,
        ctx: &mut CheckCtx,
        op: ast::BinOp,
        l: LExpr,
        r: LExpr,
        span: Span,
    ) -> LExpr {
        match infer::binop_outcome(op, l.sty, r.sty) {
            infer::BinOutcome::Error => error_lexpr(span),
            infer::BinOutcome::BoolOperand => {
                ctx.err(
                    Diagnostic::error("bool usado como int ainda não é suportado")
                        .with_primary(span, "operando bool"),
                );
                error_lexpr(span)
            }
            infer::BinOutcome::NoneOperand => {
                ctx.err(
                    Diagnostic::error("operações com None ainda não são suportadas")
                        .with_primary(span, "operando None"),
                );
                error_lexpr(span)
            }
            infer::BinOutcome::Invalid => {
                ctx.err(
                    Diagnostic::error("operandos inválidos para este operador")
                        .with_primary(span, "aqui"),
                );
                error_lexpr(span)
            }
            infer::BinOutcome::Ok {
                result,
                op: top,
                promote_left,
                promote_right,
            } => {
                let lhs = if promote_left {
                    cast_to(l.t, Ty::Float)
                } else {
                    l.t
                };
                let rhs = if promote_right {
                    cast_to(r.t, Ty::Float)
                } else {
                    r.t
                };
                LExpr {
                    sty: result,
                    t: TExpr {
                        ty: result.to_tir(),
                        kind: TExprKind::Binary {
                            op: top,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                        span,
                    },
                }
            }
        }
    }

    fn apply_unop(
        &mut self,
        ctx: &mut CheckCtx,
        op: ast::UnOp,
        operand: LExpr,
        span: Span,
    ) -> LExpr {
        if op == ast::UnOp::Not {
            let cond = self.truthy_bool(ctx, operand, span);
            return LExpr {
                sty: STy::Bool,
                t: TExpr {
                    ty: Ty::Bool,
                    kind: TExprKind::Unary {
                        op: cudra_tir::TUnOp::Not,
                        operand: Box::new(cond),
                    },
                    span,
                },
            };
        }
        match infer::unop_outcome(op, operand.sty) {
            infer::UnOutcome::Error => error_lexpr(span),
            infer::UnOutcome::BoolOperand => {
                ctx.err(
                    Diagnostic::error("bool usado como int ainda não é suportado")
                        .with_primary(span, "operando bool"),
                );
                error_lexpr(span)
            }
            infer::UnOutcome::NoneOperand => {
                ctx.err(
                    Diagnostic::error("operações com None ainda não são suportadas")
                        .with_primary(span, "operando None"),
                );
                error_lexpr(span)
            }
            infer::UnOutcome::Invalid => {
                ctx.err(
                    Diagnostic::error("operando inválido para este operador")
                        .with_primary(span, "aqui"),
                );
                error_lexpr(span)
            }
            infer::UnOutcome::Ok { result, op: None } => LExpr {
                sty: result,
                t: TExpr { span, ..operand.t },
            },
            infer::UnOutcome::Ok {
                result,
                op: Some(u),
            } => LExpr {
                sty: result,
                t: TExpr {
                    ty: result.to_tir(),
                    kind: TExprKind::Unary {
                        op: u,
                        operand: Box::new(operand.t),
                    },
                    span,
                },
            },
        }
    }

    fn resolve_read(&mut self, ctx: &mut CheckCtx, id: &ast::Ident) -> LExpr {
        if self.locals_hint.contains(&id.name) {
            if let Some((lid, sty, _)) = self.locals.get(&id.name).copied() {
                if self.defined[lid.0 as usize] {
                    return LExpr {
                        sty,
                        t: TExpr {
                            ty: sty.to_tir(),
                            kind: TExprKind::Local(lid),
                            span: id.span,
                        },
                    };
                }
            }
            ctx.err(
                Diagnostic::error(format!(
                    "a variável '{}' pode não estar definida aqui",
                    ctx.sym(id.name)
                ))
                .with_primary(id.span, "uso aqui"),
            );
            return error_lexpr(id.span);
        }
        if ctx.functions.contains_key(&id.name) || ctx.imports.names.contains_key(&id.name) {
            ctx.err(
                Diagnostic::error("funções como valores ainda não são suportadas")
                    .with_primary(id.span, "aqui"),
            );
            return error_lexpr(id.span);
        }
        let text = ctx.sym(id.name).to_string();
        if resolve::BUILTINS.contains(&text.as_str()) {
            ctx.err(
                Diagnostic::error("funções como valores ainda não são suportadas")
                    .with_primary(id.span, "aqui"),
            );
            return error_lexpr(id.span);
        }
        if text == "cudra" && ctx.imports.cudra_module_pos.is_some() {
            ctx.err(
                Diagnostic::error("usar o módulo 'cudra' como valor ainda não é suportado")
                    .with_primary(id.span, "aqui"),
            );
            return error_lexpr(id.span);
        }
        if !self.is_main && ctx.top_vars.contains(&id.name) {
            ctx.err(
                Diagnostic::error("variáveis globais não são suportadas na Fase 1")
                    .with_primary(id.span, "aqui"),
            );
            return error_lexpr(id.span);
        }
        if resolve::PY_BUILTINS.contains(&text.as_str()) {
            ctx.err(
                Diagnostic::error(format!(
                    "o builtin '{text}' ainda não é suportado na Fase 1"
                ))
                .with_primary(id.span, "aqui"),
            );
            return error_lexpr(id.span);
        }
        ctx.err(
            Diagnostic::error(format!("nome '{text}' não definido")).with_primary(id.span, "aqui"),
        );
        error_lexpr(id.span)
    }

    fn lower_expr(&mut self, ctx: &mut CheckCtx, e: &ast::Expr) -> LExpr {
        match &e.kind {
            ast::ExprKind::Int(v) => {
                if *v > i64::MAX as u64 {
                    ctx.err(
                        Diagnostic::error(
                            "inteiro fora do intervalo de 64 bits ainda não é suportado",
                        )
                        .with_primary(e.span, "literal"),
                    );
                    error_lexpr(e.span)
                } else {
                    LExpr {
                        sty: STy::Int,
                        t: TExpr {
                            ty: Ty::Int,
                            kind: TExprKind::ConstInt(*v as i64),
                            span: e.span,
                        },
                    }
                }
            }
            ast::ExprKind::Float(v) => LExpr {
                sty: STy::Float,
                t: TExpr {
                    ty: Ty::Float,
                    kind: TExprKind::ConstFloat(*v),
                    span: e.span,
                },
            },
            ast::ExprKind::Bool(b) => LExpr {
                sty: STy::Bool,
                t: TExpr {
                    ty: Ty::Bool,
                    kind: TExprKind::ConstBool(*b),
                    span: e.span,
                },
            },
            ast::ExprKind::NoneLit => LExpr {
                sty: STy::NoneTy,
                t: TExpr {
                    ty: Ty::NoneTy,
                    kind: TExprKind::ConstNone,
                    span: e.span,
                },
            },
            ast::ExprKind::Name(id) => self.resolve_read(ctx, id),
            ast::ExprKind::Unary { op, operand } => {
                if *op == ast::UnOp::Neg {
                    if let ast::ExprKind::Int(v) = &operand.kind {
                        if *v == 9223372036854775808u64 {
                            return LExpr {
                                sty: STy::Int,
                                t: TExpr {
                                    ty: Ty::Int,
                                    kind: TExprKind::ConstInt(i64::MIN),
                                    span: e.span,
                                },
                            };
                        }
                    }
                }
                let o = self.lower_expr(ctx, operand);
                self.apply_unop(ctx, *op, o, e.span)
            }
            ast::ExprKind::Binary { op, lhs, rhs } => {
                let l = self.lower_expr(ctx, lhs);
                let r = self.lower_expr(ctx, rhs);
                self.apply_binop(ctx, *op, l, r, e.span)
            }
            ast::ExprKind::Logical { op, lhs, rhs } => {
                let l = self.lower_expr(ctx, lhs);
                let r = self.lower_expr(ctx, rhs);
                if l.sty == STy::Error || r.sty == STy::Error {
                    error_lexpr(e.span)
                } else if l.sty != STy::Bool || r.sty != STy::Bool {
                    ctx.err(
                        Diagnostic::error("and/or com operandos não-bool ainda não é suportado")
                            .with_primary(e.span, "aqui"),
                    );
                    error_lexpr(e.span)
                } else {
                    let kind = match op {
                        ast::LogicOp::And => TExprKind::And(Box::new(l.t), Box::new(r.t)),
                        ast::LogicOp::Or => TExprKind::Or(Box::new(l.t), Box::new(r.t)),
                    };
                    LExpr {
                        sty: STy::Bool,
                        t: TExpr {
                            ty: Ty::Bool,
                            kind,
                            span: e.span,
                        },
                    }
                }
            }
            ast::ExprKind::Compare { first, rest } => self.lower_compare(ctx, first, rest, e.span),
            ast::ExprKind::Call { callee, args } => self.lower_call(ctx, callee, args, e.span),
            ast::ExprKind::Attr { base, .. } => {
                let _ = self.lower_expr(ctx, base);
                ctx.err(
                    Diagnostic::error("atributos ainda não são suportados")
                        .with_primary(e.span, "aqui"),
                );
                error_lexpr(e.span)
            }
            ast::ExprKind::Index { base, index } => {
                let _ = self.lower_expr(ctx, base);
                let _ = self.lower_expr(ctx, index);
                ctx.err(
                    Diagnostic::error("índices ainda não são suportados")
                        .with_primary(e.span, "aqui"),
                );
                error_lexpr(e.span)
            }
        }
    }

    fn lower_compare(
        &mut self,
        ctx: &mut CheckCtx,
        first: &ast::Expr,
        rest: &[(ast::CmpOp, ast::Expr)],
        span: Span,
    ) -> LExpr {
        let first_l = self.lower_expr(ctx, first);
        let mut prev_sty = first_l.sty;
        let mut result_sty = STy::Bool;
        let mut rest_out: Vec<(TCmp, TExpr)> = Vec::with_capacity(rest.len());
        for (op, rhs_ast) in rest {
            let rhs_l = self.lower_expr(ctx, rhs_ast);
            let rhs_sty = rhs_l.sty;
            match infer::cmp_outcome(*op, prev_sty, rhs_sty) {
                infer::CmpOutcome::Error => result_sty = STy::Error,
                infer::CmpOutcome::NoneOperand => {
                    ctx.err(
                        Diagnostic::error("operações com None ainda não são suportadas")
                            .with_primary(rhs_ast.span, "comparação com None"),
                    );
                    result_sty = STy::Error;
                }
                infer::CmpOutcome::BoolMismatch => {
                    ctx.err(
                        Diagnostic::error("bool usado como int ainda não é suportado")
                            .with_primary(rhs_ast.span, "comparação com bool"),
                    );
                    result_sty = STy::Error;
                }
                infer::CmpOutcome::Ok(tcmp) => {
                    rest_out.push((tcmp, rhs_l.t));
                }
            }
            prev_sty = rhs_sty;
        }
        if result_sty == STy::Error {
            error_lexpr(span)
        } else {
            LExpr {
                sty: STy::Bool,
                t: TExpr {
                    ty: Ty::Bool,
                    kind: TExprKind::Compare {
                        first: Box::new(first_l.t),
                        rest: rest_out,
                    },
                    span,
                },
            }
        }
    }

    fn lower_call(
        &mut self,
        ctx: &mut CheckCtx,
        callee: &ast::Expr,
        args: &[ast::Expr],
        call_span: Span,
    ) -> LExpr {
        match &callee.kind {
            ast::ExprKind::Name(id) => self.lower_call_name(ctx, id, args, call_span),
            ast::ExprKind::Attr { base, attr } => {
                self.lower_call_attr(ctx, base, attr, args, call_span)
            }
            _ => {
                let _ = self.lower_expr(ctx, callee);
                for a in args {
                    let _ = self.lower_expr(ctx, a);
                }
                ctx.err(
                    Diagnostic::error("isto não é uma função").with_primary(call_span, "chamada"),
                );
                error_lexpr(call_span)
            }
        }
    }

    fn lower_call_name(
        &mut self,
        ctx: &mut CheckCtx,
        id: &ast::Ident,
        args: &[ast::Expr],
        call_span: Span,
    ) -> LExpr {
        match ctx.sym(id.name) {
            "print" => {
                let mut out = Vec::with_capacity(args.len());
                for a in args {
                    out.push(self.lower_expr(ctx, a).t);
                }
                LExpr {
                    sty: STy::NoneTy,
                    t: TExpr {
                        ty: Ty::NoneTy,
                        kind: TExprKind::Intrinsic {
                            which: Intrinsic::Print,
                            args: out,
                        },
                        span: call_span,
                    },
                }
            }
            "int" | "float" | "bool" => {
                let text = ctx.sym(id.name).to_string();
                self.lower_conv_builtin(ctx, &text, args, call_span)
            }
            "abs" => self.lower_abs(ctx, args, call_span),
            "range" => {
                for a in args {
                    let _ = self.lower_expr(ctx, a);
                }
                ctx.err(
                    Diagnostic::error("o range() fora de um for ainda não é suportado")
                        .with_primary(call_span, "range"),
                );
                error_lexpr(call_span)
            }
            _ => self.lower_call_generic(ctx, id, args, call_span),
        }
    }

    fn lower_call_generic(
        &mut self,
        ctx: &mut CheckCtx,
        id: &ast::Ident,
        args: &[ast::Expr],
        call_span: Span,
    ) -> LExpr {
        if self.locals_hint.contains(&id.name) {
            if let Some((lid, _sty, _)) = self.locals.get(&id.name).copied() {
                if !self.defined[lid.0 as usize] {
                    ctx.err(
                        Diagnostic::error(format!(
                            "a variável '{}' pode não estar definida aqui",
                            ctx.sym(id.name)
                        ))
                        .with_primary(id.span, "uso aqui"),
                    );
                } else {
                    ctx.err(
                        Diagnostic::error(format!("'{}' não é uma função", ctx.sym(id.name)))
                            .with_primary(id.span, "chamada"),
                    );
                }
            } else {
                ctx.err(
                    Diagnostic::error(format!(
                        "a variável '{}' pode não estar definida aqui",
                        ctx.sym(id.name)
                    ))
                    .with_primary(id.span, "uso aqui"),
                );
            }
            for a in args {
                let _ = self.lower_expr(ctx, a);
            }
            return error_lexpr(call_span);
        }

        if let Some(entry) = ctx.functions.get(&id.name) {
            let fid = entry.id;
            let params = entry.params.clone();
            let ret = entry.ret;
            let ready_pos = entry.ready_pos;
            let name_span = entry.name_span;

            if self.is_main {
                if let Some(p) = self.top_pos {
                    if ready_pos > p {
                        let msg = if self.dead {
                            format!(
                                "chamar '{}' antes da sua definição em código inalcançável ainda não é suportado",
                                ctx.sym(id.name)
                            )
                        } else if self.block_depth == 0 {
                            format!("'{}' usada antes de ser definida", ctx.sym(id.name))
                        } else {
                            format!(
                                "chamar '{}' antes da sua definição dentro de um bloco ainda não é suportado",
                                ctx.sym(id.name)
                            )
                        };
                        ctx.err(
                            Diagnostic::error(msg)
                                .with_primary(call_span, "chamada aqui")
                                .with_secondary(name_span, "definida aqui"),
                        );
                        for a in args {
                            let _ = self.lower_expr(ctx, a);
                        }
                        return error_lexpr(call_span);
                    }
                }
            }

            if args.len() != params.len() {
                ctx.err(
                    Diagnostic::error(format!(
                        "chamada a '{}' com {} argumentos, esperavam-se {}",
                        ctx.sym(id.name),
                        args.len(),
                        params.len()
                    ))
                    .with_primary(call_span, "chamada"),
                );
                for a in args {
                    let _ = self.lower_expr(ctx, a);
                }
                return error_lexpr(call_span);
            }

            let mut out_args = Vec::with_capacity(args.len());
            for (a, (_, psty)) in args.iter().zip(params.iter()) {
                let al = self.lower_expr(ctx, a);
                if *psty != STy::Error && al.sty != STy::Error && al.sty != *psty {
                    ctx.err(
                        Diagnostic::error(format!(
                            "tipo incompatível no argumento de '{}': esperava-se {}, veio {}",
                            ctx.sym(id.name),
                            psty.name(),
                            al.sty.name()
                        ))
                        .with_primary(a.span, "aqui"),
                    );
                }
                out_args.push(al.t);
            }
            return LExpr {
                sty: ret,
                t: TExpr {
                    ty: ret.to_tir(),
                    kind: TExprKind::Call {
                        func: fid,
                        args: out_args,
                    },
                    span: call_span,
                },
            };
        }

        if ctx.imports.names.contains_key(&id.name) {
            let text = ctx.sym(id.name).to_string();
            return self.lower_arg_intrinsic(ctx, &text, args, call_span, id.name);
        }

        if resolve::PY_BUILTINS.contains(&ctx.sym(id.name)) {
            ctx.err(
                Diagnostic::error(format!(
                    "o builtin '{}' ainda não é suportado na Fase 1",
                    ctx.sym(id.name)
                ))
                .with_primary(id.span, "chamada"),
            );
        } else {
            ctx.err(
                Diagnostic::error(format!("nome '{}' não definido", ctx.sym(id.name)))
                    .with_primary(id.span, "aqui"),
            );
        }
        for a in args {
            let _ = self.lower_expr(ctx, a);
        }
        error_lexpr(call_span)
    }

    fn lower_call_attr(
        &mut self,
        ctx: &mut CheckCtx,
        base: &ast::Expr,
        attr: &ast::Ident,
        args: &[ast::Expr],
        call_span: Span,
    ) -> LExpr {
        if let ast::ExprKind::Name(base_id) = &base.kind {
            if ctx.sym(base_id.name) == "cudra" {
                if ctx.imports.cudra_module_pos.is_none() {
                    ctx.err(
                        Diagnostic::error("nome 'cudra' não definido")
                            .with_primary(base.span, "aqui"),
                    );
                    for a in args {
                        let _ = self.lower_expr(ctx, a);
                    }
                    return error_lexpr(call_span);
                }
                let attr_text = ctx.sym(attr.name).to_string();
                if attr_text == "arg_int" || attr_text == "arg_float" {
                    return self.lower_arg_intrinsic(
                        ctx,
                        &attr_text,
                        args,
                        call_span,
                        base_id.name,
                    );
                }
                ctx.err(
                    Diagnostic::error(format!("'cudra' não tem '{attr_text}'"))
                        .with_primary(attr.span, "aqui"),
                );
                for a in args {
                    let _ = self.lower_expr(ctx, a);
                }
                return error_lexpr(call_span);
            }
        }
        let _ = self.lower_expr(ctx, base);
        for a in args {
            let _ = self.lower_expr(ctx, a);
        }
        ctx.err(
            Diagnostic::error("atributos ainda não são suportados").with_primary(attr.span, "aqui"),
        );
        error_lexpr(call_span)
    }

    fn lower_arg_intrinsic(
        &mut self,
        ctx: &mut CheckCtx,
        text: &str,
        args: &[ast::Expr],
        call_span: Span,
        import_name: Symbol,
    ) -> LExpr {
        if self.is_main {
            if let Some(p) = self.top_pos {
                let pos = resolve::import_position(ctx, import_name);
                if pos > p {
                    let msg = if self.dead {
                        format!(
                            "usar '{text}' antes da sua importação em código inalcançável ainda não é suportado"
                        )
                    } else if self.block_depth == 0 {
                        format!("'{text}' usada antes de ser definida")
                    } else {
                        format!(
                            "usar '{text}' antes da sua importação dentro de um bloco ainda não é suportado"
                        )
                    };
                    ctx.err(Diagnostic::error(msg).with_primary(call_span, "chamada aqui"));
                    for a in args {
                        let _ = self.lower_expr(ctx, a);
                    }
                    return error_lexpr(call_span);
                }
            }
        }
        if args.len() != 1 {
            ctx.err(
                Diagnostic::error(format!("{text}() espera exatamente 1 argumento"))
                    .with_primary(call_span, "chamada"),
            );
            for a in args {
                let _ = self.lower_expr(ctx, a);
            }
            return error_lexpr(call_span);
        }
        let a = self.lower_expr(ctx, &args[0]);
        if a.sty == STy::Bool {
            ctx.err(
                Diagnostic::error("bool usado como int ainda não é suportado")
                    .with_primary(args[0].span, "aqui"),
            );
            return error_lexpr(call_span);
        }
        if a.sty != STy::Int && a.sty != STy::Error {
            ctx.err(
                Diagnostic::error(format!("{text}(): o índice tem de ser int"))
                    .with_primary(args[0].span, "aqui"),
            );
            return error_lexpr(call_span);
        }
        if a.sty == STy::Error {
            return error_lexpr(call_span);
        }
        let (which, ret) = if text == "arg_int" {
            (Intrinsic::ArgInt, STy::Int)
        } else {
            (Intrinsic::ArgFloat, STy::Float)
        };
        LExpr {
            sty: ret,
            t: TExpr {
                ty: ret.to_tir(),
                kind: TExprKind::Intrinsic {
                    which,
                    args: vec![a.t],
                },
                span: call_span,
            },
        }
    }

    fn lower_conv_builtin(
        &mut self,
        ctx: &mut CheckCtx,
        name: &str,
        args: &[ast::Expr],
        call_span: Span,
    ) -> LExpr {
        if args.is_empty() {
            let (kind, sty, ty) = match name {
                "int" => (TExprKind::ConstInt(0), STy::Int, Ty::Int),
                "float" => (TExprKind::ConstFloat(0.0), STy::Float, Ty::Float),
                _ => (TExprKind::ConstBool(false), STy::Bool, Ty::Bool),
            };
            return LExpr {
                sty,
                t: TExpr {
                    ty,
                    kind,
                    span: call_span,
                },
            };
        }
        if args.len() > 1 {
            for a in args {
                let _ = self.lower_expr(ctx, a);
            }
            ctx.err(
                Diagnostic::error(format!("{name}() aceita no máximo 1 argumento"))
                    .with_primary(call_span, "chamada"),
            );
            return error_lexpr(call_span);
        }
        let a = self.lower_expr(ctx, &args[0]);
        if a.sty == STy::NoneTy {
            ctx.err(
                Diagnostic::error("operações com None ainda não são suportadas")
                    .with_primary(args[0].span, "aqui"),
            );
            return error_lexpr(call_span);
        }
        if a.sty == STy::Error {
            return error_lexpr(call_span);
        }
        match name {
            "bool" => {
                let t = self.truthy_bool(ctx, a, call_span);
                LExpr { sty: STy::Bool, t }
            }
            "int" if a.sty == STy::Int => LExpr {
                sty: STy::Int,
                t: a.t,
            },
            "int" => LExpr {
                sty: STy::Int,
                t: cast_to(a.t, Ty::Int),
            },
            "float" if a.sty == STy::Float => LExpr {
                sty: STy::Float,
                t: a.t,
            },
            "float" => LExpr {
                sty: STy::Float,
                t: cast_to(a.t, Ty::Float),
            },
            _ => unreachable!(),
        }
    }

    fn lower_abs(&mut self, ctx: &mut CheckCtx, args: &[ast::Expr], call_span: Span) -> LExpr {
        if args.len() != 1 {
            for a in args {
                let _ = self.lower_expr(ctx, a);
            }
            ctx.err(
                Diagnostic::error("abs() espera exatamente 1 argumento")
                    .with_primary(call_span, "chamada"),
            );
            return error_lexpr(call_span);
        }
        let a = self.lower_expr(ctx, &args[0]);
        match a.sty {
            STy::Error => error_lexpr(call_span),
            STy::Bool => {
                ctx.err(
                    Diagnostic::error("bool usado como int ainda não é suportado")
                        .with_primary(args[0].span, "aqui"),
                );
                error_lexpr(call_span)
            }
            STy::NoneTy => {
                ctx.err(
                    Diagnostic::error("operações com None ainda não são suportadas")
                        .with_primary(args[0].span, "aqui"),
                );
                error_lexpr(call_span)
            }
            STy::Int => LExpr {
                sty: STy::Int,
                t: TExpr {
                    ty: Ty::Int,
                    kind: TExprKind::Intrinsic {
                        which: Intrinsic::AbsInt,
                        args: vec![a.t],
                    },
                    span: call_span,
                },
            },
            STy::Float => LExpr {
                sty: STy::Float,
                t: TExpr {
                    ty: Ty::Float,
                    kind: TExprKind::Intrinsic {
                        which: Intrinsic::AbsFloat,
                        args: vec![a.t],
                    },
                    span: call_span,
                },
            },
        }
    }

    // sombra a um builtin nosso (print/int/float/bool/abs/range) ou a um
    // nome importado (arg_int/arg_float/cudra) dentro do corpo de uma
    // função; ao nível do topo já é verificado em resolve.rs. Devolve `true`
    // quando já reportou, para o chamador não somar um segundo diagnóstico.
    fn check_local_builtin_shadow(&self, ctx: &mut CheckCtx, name: Symbol, span: Span) -> bool {
        if self.is_main {
            return false;
        }
        let text = ctx.sym(name).to_string();
        let is_import_name = ctx.imports.names.contains_key(&name)
            || (text == "cudra" && ctx.imports.cudra_module_pos.is_some());
        if resolve::BUILTINS.contains(&text.as_str()) {
            ctx.err(
                Diagnostic::error(format!(
                    "fazer sombra ao builtin '{text}' ainda não é suportado na Fase 1"
                ))
                .with_primary(span, "sombra aqui"),
            );
            true
        } else if is_import_name {
            ctx.err(
                Diagnostic::error(format!(
                    "fazer sombra ao nome importado '{text}' ainda não é suportado na Fase 1"
                ))
                .with_primary(span, "sombra aqui"),
            );
            true
        } else {
            false
        }
    }

    fn declare_for_var(&mut self, ctx: &mut CheckCtx, var: &ast::Ident) -> LocalId {
        if let Some((lid, es, _)) = self.locals.get(&var.name).copied() {
            if es != STy::Int {
                ctx.err(
                    Diagnostic::error(format!(
                        "tipo incompatível: '{}' é {}, mas o for atribui int",
                        ctx.sym(var.name),
                        es.name()
                    ))
                    .with_primary(var.span, "aqui"),
                );
            }
            lid
        } else {
            self.check_local_builtin_shadow(ctx, var.name, var.span);
            let lid = self.new_local(ctx, var.name, STy::Int);
            self.locals.insert(var.name, (lid, STy::Int, var.span));
            lid
        }
    }

    fn lower_range_args(
        &mut self,
        ctx: &mut CheckCtx,
        args: &[ast::Expr],
        span: Span,
    ) -> (TExpr, TExpr, TExpr) {
        let fallback = |s: Span| {
            let z = TExpr {
                ty: Ty::Int,
                kind: TExprKind::ConstInt(0),
                span: s,
            };
            (
                z.clone(),
                z,
                TExpr {
                    ty: Ty::Int,
                    kind: TExprKind::ConstInt(1),
                    span: s,
                },
            )
        };
        if args.is_empty() || args.len() > 3 {
            for a in args {
                let _ = self.lower_expr(ctx, a);
            }
            ctx.err(
                Diagnostic::error("range() aceita entre 1 e 3 argumentos")
                    .with_primary(span, "range"),
            );
            return fallback(span);
        }
        let mut vals = Vec::with_capacity(args.len());
        let mut ok = true;
        for a in args {
            let l = self.lower_expr(ctx, a);
            if l.sty == STy::Bool {
                ctx.err(
                    Diagnostic::error("bool usado como int ainda não é suportado")
                        .with_primary(a.span, "aqui"),
                );
                ok = false;
            } else if l.sty == STy::Error {
                ok = false;
            } else if l.sty != STy::Int {
                ctx.err(
                    Diagnostic::error("range(): os argumentos têm de ser int")
                        .with_primary(a.span, "aqui"),
                );
                ok = false;
            }
            vals.push(l.t);
        }
        if !ok {
            return fallback(span);
        }
        match vals.len() {
            1 => {
                let stop = vals.remove(0);
                (
                    TExpr {
                        ty: Ty::Int,
                        kind: TExprKind::ConstInt(0),
                        span,
                    },
                    stop,
                    TExpr {
                        ty: Ty::Int,
                        kind: TExprKind::ConstInt(1),
                        span,
                    },
                )
            }
            2 => {
                let stop = vals.remove(1);
                let start = vals.remove(0);
                (
                    start,
                    stop,
                    TExpr {
                        ty: Ty::Int,
                        kind: TExprKind::ConstInt(1),
                        span,
                    },
                )
            }
            3 => {
                let step = vals.remove(2);
                let stop = vals.remove(1);
                let start = vals.remove(0);
                (start, stop, step)
            }
            _ => unreachable!(),
        }
    }

    fn lower_for(
        &mut self,
        ctx: &mut CheckCtx,
        var: &ast::Ident,
        iter: &ast::Expr,
        body: &ast::Block,
    ) -> (Option<TStmt>, bool) {
        let is_range_call = matches!(&iter.kind, ast::ExprKind::Call{callee, ..} if matches!(&callee.kind, ast::ExprKind::Name(n) if ctx.sym(n.name) == "range"));
        let (start, stop, step) = if is_range_call {
            let ast::ExprKind::Call { args, .. } = &iter.kind else {
                unreachable!()
            };
            self.lower_range_args(ctx, args, iter.span)
        } else {
            let _ = self.lower_expr(ctx, iter);
            ctx.err(
                Diagnostic::error("o iterável de 'for' só suporta range(...); o resto ainda não é suportado nesta fase")
                    .with_primary(iter.span, "iterável"),
            );
            let z = TExpr {
                ty: Ty::Int,
                kind: TExprKind::ConstInt(0),
                span: iter.span,
            };
            (
                z.clone(),
                z,
                TExpr {
                    ty: Ty::Int,
                    kind: TExprKind::ConstInt(1),
                    span: iter.span,
                },
            )
        };

        let var_id = self.declare_for_var(ctx, var);
        let mut pre = self.defined.clone();
        self.loop_stack.push(LoopCtx {
            kind: LoopKind::Normal,
            break_snapshots: Vec::new(),
        });
        self.defined[var_id.0 as usize] = true;
        self.block_depth += 1;
        let (body_stmts, _) = self.lower_block(ctx, &body.stmts);
        self.block_depth -= 1;
        self.loop_stack.pop();
        definite::pad(&mut pre, self.tir_locals.len());
        self.defined = pre;

        (
            Some(TStmt::ForRange {
                var: var_id,
                start,
                stop,
                step,
                body: body_stmts,
            }),
            false,
        )
    }

    fn lower_while(
        &mut self,
        ctx: &mut CheckCtx,
        cond: &ast::Expr,
        body: &ast::Block,
    ) -> (Option<TStmt>, bool) {
        let infinite = matches!(&cond.kind, ast::ExprKind::Bool(true))
            || matches!(&cond.kind, ast::ExprKind::Int(v) if *v != 0);
        let cond_l = self.lower_expr(ctx, cond);
        let cond_t = self.truthy_bool(ctx, cond_l, cond.span);

        if infinite {
            self.loop_stack.push(LoopCtx {
                kind: LoopKind::Infinite,
                break_snapshots: Vec::new(),
            });
            self.block_depth += 1;
            let (body_stmts, _) = self.lower_block(ctx, &body.stmts);
            self.block_depth -= 1;
            let lc = self.loop_stack.pop().unwrap();
            let terminated = lc.break_snapshots.is_empty();
            if !terminated {
                let mut merged = lc.break_snapshots[0].clone();
                for snap in &lc.break_snapshots[1..] {
                    definite::and_into(&mut merged, snap);
                }
                definite::pad(&mut merged, self.tir_locals.len());
                self.defined = merged;
            }
            (
                Some(TStmt::While {
                    cond: cond_t,
                    body: body_stmts,
                }),
                terminated,
            )
        } else {
            let mut pre = self.defined.clone();
            self.loop_stack.push(LoopCtx {
                kind: LoopKind::Normal,
                break_snapshots: Vec::new(),
            });
            self.block_depth += 1;
            let (body_stmts, _) = self.lower_block(ctx, &body.stmts);
            self.block_depth -= 1;
            self.loop_stack.pop();
            definite::pad(&mut pre, self.tir_locals.len());
            self.defined = pre;
            (
                Some(TStmt::While {
                    cond: cond_t,
                    body: body_stmts,
                }),
                false,
            )
        }
    }

    fn lower_if(
        &mut self,
        ctx: &mut CheckCtx,
        cond: &ast::Expr,
        then: &ast::Block,
        elifs: &[(ast::Expr, ast::Block)],
        orelse: &Option<ast::Block>,
    ) -> (Option<TStmt>, bool) {
        let pre = self.defined.clone();

        self.defined = pre.clone();
        definite::pad(&mut self.defined, self.tir_locals.len());
        let cl = self.lower_expr(ctx, cond);
        let cond_t = self.truthy_bool(ctx, cl, cond.span);
        self.block_depth += 1;
        let (then_stmts, then_term) = self.lower_block(ctx, &then.stmts);
        self.block_depth -= 1;
        let then_after = self.defined.clone();

        let mut elif_results = Vec::with_capacity(elifs.len());
        for (c, b) in elifs {
            self.defined = pre.clone();
            definite::pad(&mut self.defined, self.tir_locals.len());
            let cl2 = self.lower_expr(ctx, c);
            let ct = self.truthy_bool(ctx, cl2, c.span);
            self.block_depth += 1;
            let (stmts, term) = self.lower_block(ctx, &b.stmts);
            self.block_depth -= 1;
            let after = self.defined.clone();
            elif_results.push((ct, stmts, term, after));
        }

        let (orelse_stmts, orelse_term, orelse_after) = if let Some(o) = orelse {
            self.defined = pre.clone();
            definite::pad(&mut self.defined, self.tir_locals.len());
            self.block_depth += 1;
            let (stmts, term) = self.lower_block(ctx, &o.stmts);
            self.block_depth -= 1;
            let after = self.defined.clone();
            (stmts, term, after)
        } else {
            (Vec::new(), false, pre.clone())
        };

        let mut merged = then_after;
        for (_, _, _, after) in &elif_results {
            definite::and_into(&mut merged, after);
        }
        definite::and_into(&mut merged, &orelse_after);
        definite::pad(&mut merged, self.tir_locals.len());
        self.defined = merged;

        let terminated = then_term && elif_results.iter().all(|(_, _, t, _)| *t) && orelse_term;

        let mut chain = orelse_stmts;
        for (ct, stmts, _, _) in elif_results.into_iter().rev() {
            chain = vec![TStmt::If {
                cond: ct,
                then: stmts,
                orelse: chain,
            }];
        }

        (
            Some(TStmt::If {
                cond: cond_t,
                then: then_stmts,
                orelse: chain,
            }),
            terminated,
        )
    }

    fn lower_return(
        &mut self,
        ctx: &mut CheckCtx,
        opt: &Option<ast::Expr>,
        span: Span,
    ) -> (Option<TStmt>, bool) {
        if self.is_main {
            ctx.err(Diagnostic::error("'return' fora de uma função").with_primary(span, "return"));
            if let Some(e) = opt {
                let _ = self.lower_expr(ctx, e);
            }
            return (None, true);
        }
        let (sty, texpr_opt) = match opt {
            None => (STy::NoneTy, None),
            Some(e) => {
                let l = self.lower_expr(ctx, e);
                (l.sty, Some(l.t))
            }
        };
        if self.ret_annotated {
            if sty != STy::Error && self.ret_sty != STy::Error && sty != self.ret_sty {
                ctx.err(
                    Diagnostic::error(format!(
                        "tipo de retorno incompatível: '{}' devolve {}, mas este return dá {}",
                        ctx.sym(self.cur_func_name),
                        self.ret_sty.name(),
                        sty.name()
                    ))
                    .with_primary(span, "return aqui"),
                );
            }
        } else {
            self.return_sites.push(sty);
        }
        (Some(TStmt::Return(texpr_opt)), true)
    }

    fn lower_break(&mut self, ctx: &mut CheckCtx, span: Span) -> (Option<TStmt>, bool) {
        if self.loop_stack.is_empty() {
            ctx.err(Diagnostic::error("'break' fora de um ciclo").with_primary(span, "break"));
        } else {
            let snap = self.defined.clone();
            if let Some(top) = self.loop_stack.last_mut() {
                if matches!(top.kind, LoopKind::Infinite) {
                    top.break_snapshots.push(snap);
                }
            }
        }
        (Some(TStmt::Break), true)
    }

    fn lower_continue(&mut self, ctx: &mut CheckCtx, span: Span) -> (Option<TStmt>, bool) {
        if self.loop_stack.is_empty() {
            ctx.err(
                Diagnostic::error("'continue' fora de um ciclo").with_primary(span, "continue"),
            );
        }
        (Some(TStmt::Continue), true)
    }

    fn lower_stmt(&mut self, ctx: &mut CheckCtx, s: &ast::Stmt) -> (Option<TStmt>, bool) {
        use ast::StmtKind::*;
        match &s.kind {
            Assign { target, ty, value } => {
                let v = self.lower_expr(ctx, value);
                match target {
                    ast::Target::Name(id) => {
                        let lid = self.assign_name(ctx, id, ty.as_ref(), v.sty, v.t.span);
                        (
                            Some(TStmt::Store {
                                local: lid,
                                value: v.t,
                            }),
                            false,
                        )
                    }
                    ast::Target::Index { base, index } => {
                        let _ = self.lower_expr(ctx, base);
                        let _ = self.lower_expr(ctx, index);
                        ctx.err(
                            Diagnostic::error("atribuição a índices ainda não é suportada")
                                .with_primary(s.span, "atribuição a índice"),
                        );
                        (None, false)
                    }
                }
            }
            AugAssign { target, op, value } => {
                let v = self.lower_expr(ctx, value);
                match target {
                    ast::Target::Name(id) => match self.locals.get(&id.name).copied() {
                        None => {
                            if !self.check_local_builtin_shadow(ctx, id.name, id.span) {
                                ctx.err(
                                    Diagnostic::error(format!(
                                        "a variável '{}' pode não estar definida aqui",
                                        ctx.sym(id.name)
                                    ))
                                    .with_primary(id.span, "uso aqui"),
                                );
                            }
                            (None, false)
                        }
                        Some((lid, es, _)) => {
                            if !self.defined[lid.0 as usize] {
                                ctx.err(
                                    Diagnostic::error(format!(
                                        "a variável '{}' pode não estar definida aqui",
                                        ctx.sym(id.name)
                                    ))
                                    .with_primary(id.span, "uso aqui"),
                                );
                            }
                            let lhs = LExpr {
                                sty: es,
                                t: TExpr {
                                    ty: es.to_tir(),
                                    kind: TExprKind::Local(lid),
                                    span: id.span,
                                },
                            };
                            let combined = self.apply_binop(ctx, *op, lhs, v, s.span);
                            if combined.sty != STy::Error && combined.sty != es {
                                ctx.err(
                                    Diagnostic::error(format!(
                                        "tipo incompatível: '{}' é {}, mas o resultado é {}",
                                        ctx.sym(id.name),
                                        es.name(),
                                        combined.sty.name()
                                    ))
                                    .with_primary(s.span, "aumento aqui"),
                                );
                            }
                            self.defined[lid.0 as usize] = true;
                            (
                                Some(TStmt::Store {
                                    local: lid,
                                    value: combined.t,
                                }),
                                false,
                            )
                        }
                    },
                    ast::Target::Index { base, index } => {
                        let _ = self.lower_expr(ctx, base);
                        let _ = self.lower_expr(ctx, index);
                        ctx.err(
                            Diagnostic::error("atribuição a índices ainda não é suportada")
                                .with_primary(s.span, "atribuição a índice"),
                        );
                        (None, false)
                    }
                }
            }
            For { var, iter, body } => self.lower_for(ctx, var, iter, body),
            While { cond, body } => self.lower_while(ctx, cond, body),
            If {
                cond,
                then,
                elifs,
                orelse,
            } => self.lower_if(ctx, cond, then, elifs, orelse),
            Return(opt) => self.lower_return(ctx, opt, s.span),
            Break => self.lower_break(ctx, s.span),
            Continue => self.lower_continue(ctx, s.span),
            Pass => (None, false),
            Expr(e) => {
                let v = self.lower_expr(ctx, e);
                (Some(TStmt::Expr(v.t)), false)
            }
            Import { .. } => {
                if !self.is_main {
                    ctx.err(
                        Diagnostic::error("imports dentro de funções ainda não são suportados")
                            .with_primary(s.span, "import"),
                    );
                }
                (None, false)
            }
            FromImport { .. } => {
                if !self.is_main {
                    ctx.err(
                        Diagnostic::error("imports dentro de funções ainda não são suportados")
                            .with_primary(s.span, "import"),
                    );
                }
                (None, false)
            }
        }
    }

    fn assign_name(
        &mut self,
        ctx: &mut CheckCtx,
        id: &ast::Ident,
        ann: Option<&ast::TypeExpr>,
        value_sty: STy,
        value_span: Span,
    ) -> LocalId {
        let ann_sty = ann.map(|t| resolve::resolve_type_expr(ctx, t));
        if let Some((lid, es, espan)) = self.locals.get(&id.name).copied() {
            if let Some(a) = ann_sty {
                if a != STy::Error && a != es {
                    ctx.err(
                        Diagnostic::error(format!(
                            "tipo incompatível: '{}' é {}, mas volta a ser declarada como {}",
                            ctx.sym(id.name),
                            es.name(),
                            a.name()
                        ))
                        .with_primary(id.span, "nova anotação aqui")
                        .with_secondary(
                            espan,
                            format!("'{}' declarada como {} aqui", ctx.sym(id.name), es.name()),
                        ),
                    );
                }
            }
            if value_sty != STy::Error && value_sty != es {
                ctx.err(
                    Diagnostic::error(format!(
                        "tipo incompatível: '{}' é {}, mas o valor é {}",
                        ctx.sym(id.name),
                        es.name(),
                        value_sty.name()
                    ))
                    .with_primary(value_span, format!("atribuído {}", value_sty.name()))
                    .with_secondary(
                        espan,
                        format!("'{}' declarada como {} aqui", ctx.sym(id.name), es.name()),
                    ),
                );
            }
            self.defined[lid.0 as usize] = true;
            lid
        } else {
            // Uma anotação por resolver (tipo desconhecido → `Error`) já
            // gerou o seu diagnóstico: o local fica com o tipo do valor.
            let declared = match ann_sty {
                Some(STy::Error) | None => value_sty,
                Some(a) => a,
            };
            if let Some(a) = ann_sty {
                if a != STy::Error && value_sty != STy::Error && a != value_sty {
                    ctx.err(
                        Diagnostic::error(format!(
                            "tipo incompatível: '{}' é {}, mas o valor é {}",
                            ctx.sym(id.name),
                            a.name(),
                            value_sty.name()
                        ))
                        .with_primary(value_span, format!("atribuído {}", value_sty.name())),
                    );
                }
            }
            self.check_local_builtin_shadow(ctx, id.name, id.span);
            let lid = self.new_local(ctx, id.name, declared);
            self.locals.insert(id.name, (lid, declared, id.span));
            self.defined[lid.0 as usize] = true;
            lid
        }
    }

    // Instruções depois de um terminador continuam a ser verificadas (um
    // `x = 2.5` morto depois de `x = 1` ainda é um erro de tipos), mas não
    // entram na TIR. Marcadas como `dead`, também não geram diagnósticos que
    // só valem para código que corre ("'f' usada antes de ser definida"
    // implica NameError do CPython).
    fn lower_block(&mut self, ctx: &mut CheckCtx, stmts: &[ast::Stmt]) -> (Vec<TStmt>, bool) {
        let outer_dead = self.dead;
        let mut out = Vec::new();
        let mut terminated = false;
        for s in stmts {
            if terminated {
                self.dead = true;
            }
            let (t, term) = self.lower_stmt(ctx, s);
            if !terminated {
                if let Some(ts) = t {
                    out.push(ts);
                }
                terminated = term;
            }
        }
        self.dead = outer_dead;
        (out, terminated)
    }

    fn finalize_return_type(
        &mut self,
        ctx: &mut CheckCtx,
        terminated: bool,
        func_span: Span,
    ) -> STy {
        if self.ret_annotated {
            if !terminated && self.ret_sty != STy::NoneTy && self.ret_sty != STy::Error {
                let ann = match self.ret_annot_span {
                    Some(a) => self.name_span.to(a),
                    None => self.name_span,
                };
                ctx.err(
                    Diagnostic::error(format!(
                        "a função '{}' pode terminar sem return (devolveria None)",
                        ctx.sym(self.cur_func_name)
                    ))
                    .with_primary(ann, format!("declarada com '-> {}'", self.ret_sty.name()))
                    .with_secondary(self.body_end, "a execução pode chegar ao fim aqui"),
                );
            }
            self.ret_sty
        } else {
            let mut sites = std::mem::take(&mut self.return_sites);
            if !terminated {
                sites.push(STy::NoneTy);
            }
            let mut chosen: Option<STy> = None;
            let mut inconsistent = false;
            for s in &sites {
                if *s == STy::Error {
                    continue;
                }
                match chosen {
                    None => chosen = Some(*s),
                    Some(c) if c == *s => {}
                    Some(_) => inconsistent = true,
                }
            }
            if inconsistent {
                ctx.err(
                    Diagnostic::error(format!(
                        "return inconsistente em '{}'",
                        ctx.sym(self.cur_func_name)
                    ))
                    .with_primary(func_span, "aqui"),
                );
                STy::Error
            } else {
                chosen.unwrap_or(STy::NoneTy)
            }
        }
    }
}

fn process_function(
    ctx: &mut CheckCtx,
    f: &ast::FuncDef,
    name: Symbol,
    plan: &ResolvePlan,
) -> TFunc {
    let (id, params_sty, ret_annotated, ret_sty, locals_hint, func_span) = {
        let e = &ctx.functions[&name];
        (
            e.id,
            e.params.clone(),
            e.ret_annotated,
            e.ret,
            e.locals_hint.clone(),
            e.span,
        )
    };

    let body_end = Span::new(f.body.span.file, f.body.span.hi, f.body.span.hi);
    let spans = FuncSpans {
        name: f.name.span,
        ret_annot: f.ret.as_ref().map(|t| t.span),
        body_end,
    };
    let mut fc = FuncChecker::new(false, name, ret_annotated, ret_sty, locals_hint, spans);
    let mut param_ids = Vec::with_capacity(params_sty.len());
    for (i, (pname, psty)) in params_sty.iter().enumerate() {
        let span = f.params[i].span;
        let lid = fc.new_local(ctx, *pname, *psty);
        fc.locals.insert(*pname, (lid, *psty, span));
        fc.defined[lid.0 as usize] = true;
        param_ids.push(lid);
    }

    let (tstmts, terminated) = fc.lower_block(ctx, &f.body.stmts);
    let inferred = fc.finalize_return_type(ctx, terminated, func_span);

    let final_ret = if plan.invalid_recursive.contains(&name) {
        STy::Error
    } else {
        inferred
    };
    ctx.functions.get_mut(&name).unwrap().ret = final_ret;

    TFunc {
        id,
        name: ctx.sym(name).to_string(),
        params: param_ids,
        locals: fc.tir_locals,
        ret: final_ret.to_tir(),
        body: tstmts,
    }
}

fn process_main(ctx: &mut CheckCtx, module: &ast::Module, main_id: FuncId) -> TFunc {
    let locals_hint = ctx.top_vars.clone();
    let no_span = Span::new(FileId(0), 0, 0);
    let spans = FuncSpans {
        name: no_span,
        ret_annot: None,
        body_end: no_span,
    };
    let mut fc = FuncChecker::new(true, Symbol(0), false, STy::NoneTy, locals_hint, spans);
    let mut out = Vec::new();
    let mut terminated = false;
    for (pos, item) in module.items.iter().enumerate() {
        let ast::Item::Stmt(s) = item else { continue };
        fc.top_pos = Some(pos);
        if terminated {
            fc.dead = true;
        }
        let (t, term) = fc.lower_stmt(ctx, s);
        if !terminated {
            if let Some(ts) = t {
                out.push(ts);
            }
            terminated = term;
        }
    }
    TFunc {
        id: main_id,
        name: "main".to_string(),
        params: Vec::new(),
        locals: fc.tir_locals,
        ret: Ty::NoneTy,
        body: out,
    }
}

pub(crate) fn lower_module(
    ctx: &mut CheckCtx,
    module: &ast::Module,
    plan: &ResolvePlan,
) -> Option<TModule> {
    let mut defs: HashMap<Symbol, &ast::FuncDef> = HashMap::new();
    for item in &module.items {
        if let ast::Item::Func(f) = item {
            defs.entry(f.name.name).or_insert(f);
        }
    }

    let n = ctx.func_order.len();
    let mut tir_funcs: Vec<Option<TFunc>> = (0..n).map(|_| None).collect();
    for scc in &plan.scc_order {
        for &name in scc {
            let Some(&f) = defs.get(&name) else { continue };
            let tf = process_function(ctx, f, name, plan);
            let idx = ctx.functions[&name].id.0 as usize;
            tir_funcs[idx] = Some(tf);
        }
    }

    let main_id = FuncId(n as u32);
    let main_func = process_main(ctx, module, main_id);

    if !ctx.diags.is_empty() {
        return None;
    }

    let mut funcs: Vec<TFunc> = Vec::with_capacity(n + 1);
    for f in tir_funcs {
        funcs.push(f.expect("todas as funções válidas foram processadas"));
    }
    funcs.push(main_func);

    Some(TModule {
        funcs,
        main: main_id,
    })
}
