mod common;
use common::parse_src;

// depth(name) = 1; cada nível (unário, binário, `**`) soma 1 ao máximo dos
// filhos — replica exatamente `depth()` do oráculo (tests/oracle/parser_oracle.py).
// MAX_DEPTH = 1000: depth <= 1000 aceite, depth > 1000 rejeitado.

// Um `Name` sozinho já tem depth 1 (0 filhos); cada nível `-`/`a+`/`a**`
// soma 1. Para produzir depth == `d` são precisas `d - 1` repetições.

fn neg_chain(depth: usize) -> String {
    format!("x = {}a\n", "-".repeat(depth - 1))
}

fn add_chain(depth: usize) -> String {
    format!("x = {}a\n", "a+".repeat(depth - 1))
}

fn pow_chain(depth: usize) -> String {
    format!("x = {}a\n", "a**".repeat(depth - 1))
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
    assert!(diags[0].message.contains("demasiado aninhad"));
}

#[test]
fn add_chain_depth_999_ok() {
    let (_m, diags) = parse_src(&add_chain(999));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn add_chain_depth_1000_ok() {
    let (_m, diags) = parse_src(&add_chain(1000));
    assert!(diags.is_empty(), "diags: {diags:?}");
}

#[test]
fn add_chain_depth_1001_rejected() {
    let (_m, diags) = parse_src(&add_chain(1001));
    assert_eq!(diags.len(), 1, "diags: {diags:?}");
    assert!(diags[0].message.contains("demasiado aninhad"));
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
    assert!(diags[0].message.contains("demasiado aninhad"));
}

// entradas adversárias (100 000 repetições): não pode rebentar a pilha, e
// tem de terminar com exatamente um erro de profundidade.
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
