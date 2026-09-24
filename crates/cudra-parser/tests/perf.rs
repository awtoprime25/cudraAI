mod common;
use common::parse_src;
use std::time::{Duration, Instant};

// Entradas adversárias que não podem degradar para O(n^2): cada uma tem de
// terminar em bem menos de 2s.
//
// NOTA: os casos "$" x1M e ")" x1M do ataque do oráculo (tests/oracle/parser_attack.py)
// não estão aqui — são lentos por um custo quadrático dentro do próprio
// cudra-lexer (reproduzível com `dump_tokens`, sem qualquer código do
// parser envolvido), fora do âmbito desta Parte B. Ver o relatório final.

const LIMIT: Duration = Duration::from_secs(2);

fn assert_fast(name: &str, src: &str) {
    let start = Instant::now();
    let (_m, _diags) = parse_src(src);
    let elapsed = start.elapsed();
    assert!(
        elapsed < LIMIT,
        "{name} demorou {elapsed:?} (limite {LIMIT:?})"
    );
}

#[test]
fn perf_1mb_repeated_assign() {
    let src = "x = 1\n".repeat(170_000);
    assert_fast("lines_1MB", &src);
}

#[test]
fn perf_add_chain_500k() {
    let src = format!("x = {}a\n", "a+".repeat(500_000));
    assert_fast("add_500k", &src);
}

#[test]
fn perf_nested_parens_200_deep_repeated() {
    let one = format!("x = {}a{}\n", "(".repeat(199), ")".repeat(199));
    let src = one.repeat(2000);
    assert_fast("parens_200_rep", &src);
}

#[test]
fn perf_call_chain_100k() {
    let src = format!("x = a{}\n", "()".repeat(100_000));
    assert_fast("call_100k", &src);
}

#[test]
fn perf_attr_chain_100k() {
    let src = format!("x = a{}\n", ".b".repeat(100_000));
    assert_fast("attr_100k", &src);
}

#[test]
fn perf_index_chain_100k() {
    let src = format!("x = a{}\n", "[0]".repeat(100_000));
    assert_fast("idx_100k", &src);
}

#[test]
fn perf_elif_chain_100k() {
    let src = format!("if a:\n    pass\n{}", "elif b:\n    pass\n".repeat(100_000));
    assert_fast("elif_100k", &src);
}

#[test]
fn perf_args_300k() {
    let src = format!("f({})\n", "a,".repeat(300_000));
    assert_fast("args_300k", &src);
}

#[test]
fn perf_semis_300k() {
    let src = "x = 1; ".repeat(300_000) + "\n";
    assert_fast("semis_300k", &src);
}
