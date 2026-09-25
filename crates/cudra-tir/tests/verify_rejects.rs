use cudra_span::{FileId, Span};
use cudra_tir::{
    Func, FuncId, Intrinsic, Local, LocalId, Module, TBinOp, TCmp, TExpr, TExprKind, TStmt, TUnOp,
    Ty,
};

fn sp() -> Span {
    Span::new(FileId(0), 0, 1)
}

fn e(ty: Ty, kind: TExprKind) -> TExpr {
    TExpr {
        ty,
        kind,
        span: sp(),
    }
}

fn const_int(v: i64) -> TExpr {
    e(Ty::Int, TExprKind::ConstInt(v))
}

fn func(
    id: u32,
    name: &str,
    params: Vec<LocalId>,
    locals: Vec<Local>,
    ret: Ty,
    body: Vec<TStmt>,
) -> Func {
    Func {
        id: FuncId(id),
        name: name.to_string(),
        params,
        locals,
        ret,
        body,
    }
}

fn module(funcs: Vec<Func>, main: u32) -> Module {
    Module {
        funcs,
        main: FuncId(main),
    }
}

fn base_main(ret: Ty, body: Vec<TStmt>) -> Func {
    func(0, "main", vec![], vec![], ret, body)
}

#[test]
fn valid_module_is_accepted() {
    let m = module(
        vec![base_main(
            Ty::NoneTy,
            vec![TStmt::Expr(e(
                Ty::NoneTy,
                TExprKind::Intrinsic {
                    which: Intrinsic::Print,
                    args: vec![const_int(1)],
                },
            ))],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_ok());
}

#[test]
fn main_func_id_out_of_bounds() {
    let m = module(vec![base_main(Ty::NoneTy, vec![])], 1);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn func_index_does_not_match_func_id() {
    let f = func(5, "main", vec![], vec![], Ty::NoneTy, vec![]);
    let m = module(vec![f], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn param_local_id_out_of_bounds() {
    let f = func(0, "main", vec![LocalId(0)], vec![], Ty::NoneTy, vec![]);
    let m = module(vec![f], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn store_value_type_mismatch() {
    let locals = vec![Local {
        name: "x".to_string(),
        ty: Ty::Int,
    }];
    let body = vec![TStmt::Store {
        local: LocalId(0),
        value: e(Ty::Bool, TExprKind::ConstBool(true)),
    }];
    let m = module(vec![func(0, "main", vec![], locals, Ty::NoneTy, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn store_local_id_out_of_bounds() {
    let body = vec![TStmt::Store {
        local: LocalId(0),
        value: const_int(1),
    }];
    let m = module(vec![func(0, "main", vec![], vec![], Ty::NoneTy, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn for_range_var_not_int() {
    let locals = vec![Local {
        name: "v".to_string(),
        ty: Ty::Bool,
    }];
    let body = vec![TStmt::ForRange {
        var: LocalId(0),
        start: const_int(0),
        stop: const_int(1),
        step: const_int(1),
        body: vec![],
    }];
    let m = module(vec![func(0, "main", vec![], locals, Ty::NoneTy, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn for_range_stop_not_int() {
    let locals = vec![Local {
        name: "v".to_string(),
        ty: Ty::Int,
    }];
    let body = vec![TStmt::ForRange {
        var: LocalId(0),
        start: const_int(0),
        stop: e(Ty::Float, TExprKind::ConstFloat(1.0)),
        step: const_int(1),
        body: vec![],
    }];
    let m = module(vec![func(0, "main", vec![], locals, Ty::NoneTy, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn while_cond_not_bool() {
    let body = vec![TStmt::While {
        cond: const_int(1),
        body: vec![],
    }];
    let m = module(vec![func(0, "main", vec![], vec![], Ty::NoneTy, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn if_cond_not_bool() {
    let body = vec![TStmt::If {
        cond: const_int(1),
        then: vec![],
        orelse: vec![],
    }];
    let m = module(vec![func(0, "main", vec![], vec![], Ty::NoneTy, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn return_value_type_mismatch() {
    let body = vec![TStmt::Return(Some(const_int(1)))];
    let m = module(vec![func(0, "main", vec![], vec![], Ty::Bool, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn return_none_but_ret_type_not_none() {
    let body = vec![TStmt::Return(None)];
    let m = module(vec![func(0, "main", vec![], vec![], Ty::Int, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn break_outside_loop_rejected() {
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Break],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn continue_outside_loop_rejected() {
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Continue],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn break_inside_while_is_accepted() {
    let body = vec![TStmt::While {
        cond: e(Ty::Bool, TExprKind::ConstBool(true)),
        body: vec![TStmt::Break],
    }];
    let m = module(vec![func(0, "main", vec![], vec![], Ty::NoneTy, body)], 0);
    assert!(cudra_tir::verify(&m).is_ok());
}

#[test]
fn local_expr_id_out_of_bounds() {
    let body = vec![TStmt::Expr(e(Ty::Int, TExprKind::Local(LocalId(0))))];
    let m = module(vec![func(0, "main", vec![], vec![], Ty::NoneTy, body)], 0);
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn binary_operand_type_mismatch() {
    let expr = e(
        Ty::Int,
        TExprKind::Binary {
            op: TBinOp::IAdd,
            lhs: Box::new(const_int(1)),
            rhs: Box::new(e(Ty::Float, TExprKind::ConstFloat(1.0))),
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn unary_operand_type_mismatch() {
    let expr = e(
        Ty::Int,
        TExprKind::Unary {
            op: TUnOp::Not,
            operand: Box::new(const_int(1)),
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn compare_with_no_comparisons_rejected() {
    let expr = e(
        Ty::Bool,
        TExprKind::Compare {
            first: Box::new(const_int(1)),
            rest: vec![],
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn compare_cmp_variant_type_mismatch() {
    let expr = e(
        Ty::Bool,
        TExprKind::Compare {
            first: Box::new(const_int(1)),
            rest: vec![(TCmp::F(cudra_tir::CmpOp::Lt), const_int(2))],
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn and_with_non_bool_operand_rejected() {
    let expr = e(
        Ty::Bool,
        TExprKind::And(
            Box::new(const_int(1)),
            Box::new(e(Ty::Bool, TExprKind::ConstBool(true))),
        ),
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn cast_invalid_pair_rejected() {
    let expr = e(
        Ty::NoneTy,
        TExprKind::Cast {
            value: Box::new(const_int(1)),
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn cast_identity_pair_rejected() {
    let expr = e(
        Ty::Int,
        TExprKind::Cast {
            value: Box::new(const_int(1)),
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn cast_valid_pair_is_accepted() {
    let expr = e(
        Ty::Float,
        TExprKind::Cast {
            value: Box::new(const_int(1)),
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_ok());
}

#[test]
fn call_func_id_out_of_bounds() {
    let expr = e(
        Ty::Int,
        TExprKind::Call {
            func: FuncId(1),
            args: vec![],
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn call_arity_mismatch() {
    let callee = func(
        1,
        "f",
        vec![LocalId(0)],
        vec![Local {
            name: "n".to_string(),
            ty: Ty::Int,
        }],
        Ty::Int,
        vec![TStmt::Return(Some(e(
            Ty::Int,
            TExprKind::Local(LocalId(0)),
        )))],
    );
    let call = e(
        Ty::Int,
        TExprKind::Call {
            func: FuncId(1),
            args: vec![],
        },
    );
    let m = module(
        vec![
            func(
                0,
                "main",
                vec![],
                vec![],
                Ty::NoneTy,
                vec![TStmt::Expr(call)],
            ),
            callee,
        ],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn call_arg_type_mismatch() {
    let callee = func(
        1,
        "f",
        vec![LocalId(0)],
        vec![Local {
            name: "n".to_string(),
            ty: Ty::Int,
        }],
        Ty::Int,
        vec![TStmt::Return(Some(e(
            Ty::Int,
            TExprKind::Local(LocalId(0)),
        )))],
    );
    let call = e(
        Ty::Int,
        TExprKind::Call {
            func: FuncId(1),
            args: vec![e(Ty::Bool, TExprKind::ConstBool(true))],
        },
    );
    let m = module(
        vec![
            func(
                0,
                "main",
                vec![],
                vec![],
                Ty::NoneTy,
                vec![TStmt::Expr(call)],
            ),
            callee,
        ],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn intrinsic_wrong_arg_count_rejected() {
    let expr = e(
        Ty::Int,
        TExprKind::Intrinsic {
            which: Intrinsic::AbsInt,
            args: vec![],
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn intrinsic_wrong_arg_type_rejected() {
    let expr = e(
        Ty::Int,
        TExprKind::Intrinsic {
            which: Intrinsic::AbsInt,
            args: vec![e(Ty::Float, TExprKind::ConstFloat(1.0))],
        },
    );
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}

#[test]
fn annotated_type_does_not_match_computed_type() {
    let expr = e(Ty::Float, TExprKind::ConstInt(1));
    let m = module(
        vec![func(
            0,
            "main",
            vec![],
            vec![],
            Ty::NoneTy,
            vec![TStmt::Expr(expr)],
        )],
        0,
    );
    assert!(cudra_tir::verify(&m).is_err());
}
