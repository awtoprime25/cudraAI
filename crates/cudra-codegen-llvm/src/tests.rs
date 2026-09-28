use cudra_span::{FileId, Span};
use cudra_tir::{CmpOp, Func, FuncId, LocalId, Module, TBinOp, TCmp, TExpr, TExprKind, TStmt, Ty};

use crate::{emit_llvm_ir, emit_object, Options};

fn span() -> Span {
    Span::new(FileId(0), 0, 0)
}

fn int_const(v: i64) -> TExpr {
    TExpr {
        ty: Ty::Int,
        kind: TExprKind::ConstInt(v),
        span: span(),
    }
}

/// Um modulo com o `main` minimo e uma funcao por cada nome dado, cada uma a
/// devolver `1`.
fn module_with(names: &[&str]) -> Module {
    let mut funcs = vec![Func {
        id: FuncId(0),
        name: "main".to_string(),
        params: Vec::new(),
        locals: Vec::new(),
        ret: Ty::NoneTy,
        body: Vec::new(),
    }];
    for (i, name) in names.iter().enumerate() {
        funcs.push(Func {
            id: FuncId(i as u32 + 1),
            name: (*name).to_string(),
            params: Vec::new(),
            locals: Vec::new(),
            ret: Ty::Int,
            body: vec![TStmt::Return(Some(int_const(1)))],
        });
    }
    Module {
        funcs,
        main: FuncId(0),
    }
}

fn symbols(ir: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, _) in ir.match_indices("@cdr_") {
        let rest = &ir[i + 1..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        out.push(rest[..end].to_string());
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn user_symbols_never_collide() {
    let names = [
        "main",
        "printf",
        "cudra_panic",
        "exit",
        "a_b",
        "a_5fb",
        "A",
        "a",
        "ação",
        "f",
        "f1",
    ];
    let ir = emit_llvm_ir(&module_with(&names), &Options { release: false }).unwrap();
    let syms = symbols(&ir);
    assert_eq!(
        syms.len(),
        names.len(),
        "símbolos repetidos: {syms:?}\n{ir}"
    );
    // o `main` do utilizador nao colide com o @main gerado
    assert!(ir.contains("@cdr_main("), "{ir}");
    assert!(ir.contains("@main("), "{ir}");
    // o escape tem de ser injetivo, mesmo para nomes parecidos
    assert!(syms.contains(&"cdr_a_5fb".to_string()), "{syms:?}");
    assert!(syms.contains(&"cdr_a_5f5fb".to_string()), "{syms:?}");
    assert!(syms.contains(&"cdr_a_c3_a7_c3_a3o".to_string()), "{syms:?}");
    // um nome so de ASCII continua legivel
    assert!(syms.contains(&"cdr_printf".to_string()), "{syms:?}");
    assert!(syms.contains(&"cdr_cudra_5fpanic".to_string()), "{syms:?}");
}

#[test]
fn emits_a_verifiable_empty_program() {
    let ir = emit_llvm_ir(&module_with(&[]), &Options { release: false }).unwrap();
    assert!(ir.contains("@main"), "{ir}");
    assert!(ir.contains("declare void @cudra_rt_init"), "{ir}");
    assert!(ir.contains("define i32 @main"), "{ir}");
}

#[test]
fn emits_object_bytes_for_both_profiles() {
    for release in [false, true] {
        let bytes = emit_object(&module_with(&["f"]), &Options { release }).unwrap();
        assert!(
            bytes.len() > 64,
            "objeto demasiado pequeno em release={release}"
        );
    }
}

#[test]
fn reachable_end_of_an_annotated_function_is_unreachable() {
    // a sema garante que o fim do corpo nao e alcancavel: o codegen tem de
    // fechar o bloco com `unreachable` e o modulo LLVM tem de verificar
    let mut m = module_with(&["f"]);
    m.funcs[1].body = Vec::new();
    let ir = emit_llvm_ir(&m, &Options { release: false }).unwrap();
    assert!(ir.contains("unreachable"), "{ir}");
}

fn local(i: usize) -> TExpr {
    TExpr {
        ty: Ty::Int,
        kind: TExprKind::Local(LocalId(i as u32)),
        span: span(),
    }
}

fn bin(op: TBinOp, lhs: TExpr, rhs: TExpr) -> TExpr {
    TExpr {
        ty: Ty::Int,
        kind: TExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        span: span(),
    }
}

/// `v{i} = v{i-1} + 1` seguido de `if v{i} > 5: v{i} = v{i} - 1`: cada soma e
/// cada subtracao traz uma verificacao de overflow, e cada `if` nao tem `else`.
fn module_de_verificacoes(n: usize) -> Module {
    let locals = (0..n)
        .map(|i| cudra_tir::Local {
            name: format!("v{i}"),
            ty: Ty::Int,
        })
        .collect();
    let mut body = vec![TStmt::Store {
        local: LocalId(0),
        value: int_const(1),
    }];
    for i in 1..n {
        body.push(TStmt::Store {
            local: LocalId(i as u32),
            value: bin(TBinOp::IAdd, local(i - 1), int_const(1)),
        });
    }
    for i in 0..n {
        body.push(TStmt::If {
            cond: TExpr {
                ty: Ty::Bool,
                kind: TExprKind::Compare {
                    first: Box::new(local(i)),
                    rest: vec![(TCmp::I(CmpOp::Gt), int_const(5))],
                },
                span: span(),
            },
            then: vec![TStmt::Store {
                local: LocalId(i as u32),
                value: bin(TBinOp::ISub, local(i), int_const(1)),
            }],
            orelse: Vec::new(),
        });
    }
    Module {
        funcs: vec![Func {
            id: FuncId(0),
            name: "main".to_string(),
            params: Vec::new(),
            locals,
            ret: Ty::NoneTy,
            body,
        }],
        main: FuncId(0),
    }
}

/// Os blocos que o codegen cria por instrucao nao podem multiplicar-se: e o
/// numero de blocos da funcao que faz o pipeline de maquina do LLVM (e a
/// promocao de allocas) degenerar numa unica funcao grande. Um bloco de panico
/// por verificacao dava `n` blocos de panico para `n` verificacoes.
#[test]
fn um_bloco_de_panico_por_codigo_e_nenhum_else_vazio() {
    let n = 300;
    let ir = emit_llvm_ir(&module_de_verificacoes(n), &Options { release: false }).unwrap();
    let panicos = ir.matches("\npanic").count();
    assert!(
        panicos <= 8,
        "{panicos} blocos de panico para {n} verificacoes (deviam ser um por codigo)"
    );
    let elses = ir.matches("if.else").count();
    assert_eq!(
        elses, 0,
        "um `if` sem `else` criou {elses} blocos de ramo vazio"
    );
}

#[test]
fn void_function_ends_with_a_void_return() {
    let mut m = module_with(&["f"]);
    m.funcs[1].ret = Ty::NoneTy;
    m.funcs[1].body = Vec::new();
    let ir = emit_llvm_ir(&m, &Options { release: false }).unwrap();
    assert!(ir.contains("ret void"), "{ir}");
}
