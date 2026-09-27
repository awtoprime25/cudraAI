use cudra_span::{FileId, Span};
use cudra_tir::{Func, FuncId, Module, TExpr, TExprKind, TStmt, Ty};

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

#[test]
fn void_function_ends_with_a_void_return() {
    let mut m = module_with(&["f"]);
    m.funcs[1].ret = Ty::NoneTy;
    m.funcs[1].body = Vec::new();
    let ir = emit_llvm_ir(&m, &Options { release: false }).unwrap();
    assert!(ir.contains("ret void"), "{ir}");
}
