// Desempenho da análise semântica (lexer + parser + sema) nas entradas
// adversárias que a fizeram degenerar em O(n²): o conjunto de locais já
// definidos era clonado e intersectado a cada ramo de `if`/`elif`/`else` e a
// cada ciclo, custando O(n.º de locais) por ramo. Cada caso tem de ficar bem
// abaixo do limite, mesmo em `cargo test` (debug); o quadrático que isto
// trava estava em dezenas de segundos.

use std::time::{Duration, Instant};

use cudra_lexer::lex;
use cudra_parser::parse;
use cudra_span::{Interner, SourceMap};

const LIMIT: Duration = Duration::from_secs(2);

// Corre numa thread com pilha grande, como o `cudrac` e o `dump_ast`.
fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(f)
        .expect("falha ao criar thread de pilha grande")
        .join()
        .expect("thread de pilha grande entrou em pânico")
}

/// Mede só a sema (lexer e parser ficam fora do cronómetro) e devolve o
/// número de diagnósticos que ela emitiu.
fn sema_time(src: &str) -> (Duration, usize) {
    let src = src.to_string();
    on_big_stack(move || {
        let mut sources = SourceMap::new();
        let file = sources.add("t.cdr".to_string(), src.clone());
        let (tokens, lex_diags) = lex(file, &src);
        assert!(
            lex_diags.is_empty(),
            "a entrada do teste não devia ter erros léxicos"
        );
        let mut interner = Interner::new();
        let (module, parse_diags) = parse(file, &src, &tokens, &mut interner);
        assert!(
            parse_diags.is_empty(),
            "a entrada do teste não devia ter erros de sintaxe"
        );
        let start = Instant::now();
        let ndiags = match cudra_sema::check(&module, &interner) {
            Ok(_) => 0,
            Err(diags) => diags.len(),
        };
        (start.elapsed(), ndiags)
    })
}

fn assert_fast(name: &str, src: &str) {
    let (elapsed, ndiags) = sema_time(src);
    eprintln!("perf sema {name}: {elapsed:?}");
    assert_eq!(ndiags, 0, "{name}: esperava 0 diagnósticos, veio {ndiags}");
    assert!(
        elapsed < LIMIT,
        "{name} demorou {elapsed:?} (limite {LIMIT:?})"
    );
}

// Variáveis de topo todas distintas: indexar os locais por nome era uma
// procura linear sobre a lista toda, O(n²) no `main`. 100 mil dão ~0,7 s
// (eram ~60 s).
#[test]
fn perf_100k_top_vars() {
    let mut src = String::new();
    for i in 0..100_000 {
        src.push_str(&format!("x{i} = 1\n"));
    }
    src.push_str("print(x0)\n");
    assert_fast("top_vars_100k", &src);
}

// Muitas variáveis e muitos `if`s: cada ramo copiava e intersectava o
// conjunto de locais definidos. 40 mil de cada davam ~22 s; agora ~0,8 s.
#[test]
fn perf_40k_vars_40k_ifs() {
    let n = 40_000;
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("v{i} = {i}\n"));
    }
    for i in 0..n {
        src.push_str(&format!("if v{i} > 5:\n    v{i} = 1\n"));
    }
    src.push_str("print(v0)\n");
    assert_fast("vars_ifs_40k", &src);
}

// Cadeia `if`/`elif`/`else` muito longa: cada ramo restaurava o estado e
// copiava o conjunto de locais definidos, e o merge final intersectava-o.
// Com n variáveis e n ramos, eram O(n²) cópias de n booleanos.
#[test]
fn perf_elif_chain_else() {
    let n = 40_000;
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("v{i} = {i}\n"));
    }
    src.push_str("if v0 > 5:\n    z = 0\n");
    for i in 1..n {
        src.push_str(&format!("elif v{i} > 5:\n    z = {i}\n"));
    }
    src.push_str("else:\n    z = 99\nprint(z)\n");
    assert_fast("elif_chain_else_40k", &src);
}

// Cadeia `if`/`elif` sem `else`: há um caminho que não define nada, o que
// obriga a intersecção a esvaziar o conjunto de locais definidos.
#[test]
fn perf_elif_chain_no_else() {
    let n = 40_000;
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("v{i} = {i}\n"));
    }
    src.push_str("if v0 > 5:\n    v0 = 1\n");
    for i in 1..n {
        src.push_str(&format!("elif v{i} > 5:\n    v{i} = 1\n"));
    }
    src.push_str("print(v0)\n");
    assert_fast("elif_chain_no_else_40k", &src);
}

// `while True` com muitos `break`: cada `break` guardava um instantâneo
// completo dos locais definidos e, à saída do ciclo, eles eram todos
// intersectados — O(n.º de breaks × n.º de locais).
#[test]
fn perf_while_true_many_breaks() {
    let n = 40_000;
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("v{i} = {i}\n"));
    }
    src.push_str("while True:\n");
    for i in 0..n {
        src.push_str(&format!(
            "    if v{i} > 5:\n        b{i} = 0\n        break\n"
        ));
    }
    src.push_str("print(v0)\n");
    assert_fast("while_true_breaks_40k", &src);
}
