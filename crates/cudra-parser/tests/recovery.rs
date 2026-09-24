mod common;
use common::parse_src;

// Um erro por instrução falhada: a instrução seguinte, no mesmo nível,
// continua a ser parseada.

#[test]
fn class_body_then_sibling_stmt_one_error() {
    // `class` ainda não é suportado: a instrução inteira é descartada (não
    // produz item), só a irmã seguinte sobrevive — como em while/for-else.
    let (module, diags) = parse_src("class A:\n    x = 1\n    def f(self): pass\nx = 1\n");
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(module.items.len(), 1, "items: {module:?}");
}

#[test]
fn try_except_finally_one_error() {
    let (module, diags) = parse_src(
        "try:\n    x = 1\nexcept E:\n    pass\nelse:\n    y = 1\nfinally:\n    z = 1\nw = 2\n",
    );
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(module.items.len(), 1, "items: {module:?}");
}

#[test]
fn if_is_with_elif_else_one_error() {
    let (module, diags) =
        parse_src("if a is b:\n    x = 1\nelif c:\n    y = 2\nelse:\n    z = 3\nw = 1\n");
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(module.items.len(), 1, "items: {module:?}");
}

#[test]
fn while_else_one_error() {
    let (module, diags) = parse_src("while a:\n    pass\nelse:\n    pass\nx = 1\n");
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(module.items.len(), 1, "items: {module:?}");
}

#[test]
fn for_else_one_error() {
    let (module, diags) = parse_src("for x in y:\n    pass\nelse:\n    pass\nz = 1\n");
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(module.items.len(), 1, "items: {module:?}");
}

// erro dentro de um corpo aninhado só salta essa instrução interior, não
// faz cascata para fora do bloco.
#[test]
fn error_inside_nested_body_does_not_cascade() {
    let (module, diags) = parse_src("if a:\n    def g():\n        pass\nelse:\n    x = 1\n");
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(module.items.len(), 1, "items: {module:?}");
    let cudra_ast::Item::Stmt(s) = &module.items[0] else {
        panic!("esperava-se um Stmt de topo");
    };
    let cudra_ast::StmtKind::If { then, orelse, .. } = &s.kind else {
        panic!("esperava-se If");
    };
    assert_eq!(then.stmts.len(), 0, "corpo do if: {then:?}");
    assert_eq!(
        orelse.as_ref().map(|b| b.stmts.len()),
        Some(1),
        "corpo do else: {orelse:?}"
    );
}

#[test]
fn nested_def_inside_def_one_error() {
    let (module, diags) = parse_src("def f():\n    def g():\n        pass\n    return 1\n");
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(module.items.len(), 1, "items: {module:?}");
}

#[test]
fn stray_dedent_no_infinite_loop() {
    let (_module, diags) = parse_src("pass\n    x\n");
    assert!(!diags.is_empty());
}

#[test]
fn juxtaposed_statements_without_separator_is_one_error() {
    // continuação de linha ('\') junta duas instruções sem separador: é um
    // erro, não duas instruções silenciosamente aceites.
    let (module, diags) = parse_src("x = 1 \\\ny = 2\n");
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert_eq!(module.items.len(), 1, "items: {module:?}");
}
