mod common;
use common::parse_src;

// depth(name) = 1; cada nível de aninhamento REAL (unário, `**` à direita,
// parênteses, chamadas) soma 1 ao máximo dos filhos. MAX_DEPTH = 1000:
// depth <= 1000 aceite, depth > 1000 rejeitado.
//
// Cadeias associativas à esquerda construídas em ciclo (`and`/`or` e os
// operadores binários — `+`, `*`, `|`, etc.) NÃO contam para esta
// profundidade: o parser não recorre por elemento, e não têm limite de
// tamanho (ver ronda 2 da parte D).

// Um `Name` sozinho já tem depth 1 (0 filhos); cada nível `-`/`a**` soma 1.
// Para produzir depth == `d` são precisas `d - 1` repetições.

fn neg_chain(depth: usize) -> String {
    format!("x = {}a\n", "-".repeat(depth - 1))
}

fn add_chain(len: usize) -> String {
    format!("x = {}a\n", "a+".repeat(len - 1))
}

fn pow_chain(depth: usize) -> String {
    format!("x = {}a\n", "a**".repeat(depth - 1))
}

fn and_chain(len: usize) -> String {
    format!("x = {}True\n", "True and ".repeat(len - 1))
}

#[test]
fn neg_chain_depth_999_ok() {
    let (_m, diags) = parse_src(&neg_chain(999));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn neg_chain_depth_1000_ok() {
    let (_m, diags) = parse_src(&neg_chain(1000));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn neg_chain_depth_1001_rejected() {
    let (_m, diags) = parse_src(&neg_chain(1001));
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert!(diags[0].message.contains("ainda não são suportadas"));
}

#[test]
fn add_chain_len_999_ok() {
    let (_m, diags) = parse_src(&add_chain(999));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn add_chain_len_1000_ok() {
    let (_m, diags) = parse_src(&add_chain(1000));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

// cadeia associativa à esquerda: já não conta para a profundidade
// estrutural (ver comentário no topo do ficheiro), por isso >1000 termos
// continua a ser aceite.
#[test]
fn add_chain_len_1001_ok() {
    let (_m, diags) = parse_src(&add_chain(1001));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn and_chain_len_1001_ok() {
    let (_m, diags) = parse_src(&and_chain(1001));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn pow_chain_depth_999_ok() {
    let (_m, diags) = parse_src(&pow_chain(999));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn pow_chain_depth_1000_ok() {
    let (_m, diags) = parse_src(&pow_chain(1000));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn pow_chain_depth_1001_rejected() {
    let (_m, diags) = parse_src(&pow_chain(1001));
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert!(diags[0].message.contains("ainda não são suportadas"));
}

// entradas adversárias (100 000 repetições): não pode rebentar a pilha, e
// tem de terminar com exatamente um erro de profundidade (aninhamento
// real) ou zero erros (cadeia associativa à esquerda, sem limite).
#[test]
fn neg_chain_100k_no_stack_overflow() {
    let (_m, diags) = parse_src(&neg_chain(100_000));
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
}

#[test]
fn not_chain_100k_no_stack_overflow() {
    let src = format!("x = {}a\n", "not ".repeat(100_000));
    let (_m, diags) = parse_src(&src);
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
}

#[test]
fn pow_chain_100k_no_stack_overflow() {
    let (_m, diags) = parse_src(&pow_chain(100_000));
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
}

#[test]
fn add_chain_100k_no_stack_overflow_and_accepted() {
    let (m, diags) = parse_src(&add_chain(100_000));
    assert!(diags.is_empty(), "diags: {diags:?}");
    common::drop_on_big_stack(m);
}

#[test]
fn and_chain_100k_no_stack_overflow_and_accepted() {
    let (m, diags) = parse_src(&and_chain(100_000));
    assert!(diags.is_empty(), "diags: {diags:?}");
    common::drop_on_big_stack(m);
}
