use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use cudra_driver::check_source;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_cudrac")
}

fn write_tmp(name: &str, src: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("cudrac_perf_{}_{}.cdr", std::process::id(), name));
    fs::write(&path, src).unwrap();
    path
}

// Em `cargo test` normal (debug) o `cudrac` não está otimizado e estes
// programas facilmente passam de 2s sem que isso signifique nada; o limite
// só é imposto em release (como corre `cargo test --release`).
fn check_under_2s(name: &str, src: &str) {
    let path = write_tmp(name, src);
    let t0 = Instant::now();
    let out = Command::new(bin())
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    let dt = t0.elapsed();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{name}: esperava-se exit=0, veio {:?}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("perf {name}: {dt:?}");
    if !cfg!(debug_assertions) {
        assert!(
            dt < Duration::from_secs(2),
            "{name}: demorou {dt:?}, esperava-se <2s"
        );
    }
    let _ = fs::remove_file(&path);
}

#[test]
fn perf_100k_lines_increment() {
    let mut src = String::from("x = 0\n");
    for _ in 0..100_000 {
        src.push_str("x = x + 1\n");
    }
    src.push_str("print(x)\n");
    check_under_2s("100k_lines_increment", &src);
}

#[test]
fn perf_10k_function_chain() {
    let mut src = String::new();
    for i in 0..10_000 {
        src.push_str(&format!(
            "def f{i}(n: int) -> int:\n    return f{}(n)\n",
            i + 1
        ));
    }
    src.push_str("def f10000(n: int) -> int:\n    return n\n");
    src.push_str("print(f0(1))\n");
    check_under_2s("10k_function_chain", &src);
}

#[test]
fn perf_100k_function_chain() {
    let mut src = String::new();
    for i in 0..100_000 {
        src.push_str(&format!(
            "def f{i}(n: int) -> int:\n    return f{}(n)\n",
            i + 1
        ));
    }
    src.push_str("def f100000(n: int) -> int:\n    return n\n");
    src.push_str("print(f0(1))\n");
    check_under_2s("100k_function_chain", &src);
}

#[test]
fn perf_1000_level_nested_expr() {
    // o parser recusa a partir de 999 níveis de profundidade (proteção
    // contra stack overflow); 998 operadores + 1 folha fica mesmo no limite.
    let mut src = String::from("x = 1\nprint(");
    for _ in 0..998 {
        src.push('-');
    }
    src.push_str("x)\n");
    check_under_2s("1000_level_nested_expr", &src);
}

// --- Entradas adversárias constantes, usadas pelos testes de pipeline (que
// param no `check`, onde as constantes são irrelevantes). As três primeiras
// foram as que fizeram a sema degenerar em O(n²): uma cadeia
// `if`/`elif`/.../`else` longa (cada ramo restaurava e copiava o conjunto de
// locais definidos), a mesma cadeia sem `else` (o caminho que não define nada
// obriga a intersecção a esvaziar) e um `while True` com muitos `break` (um
// instantâneo por `break`, intersectados à saída do ciclo). Os testes de build
// usam as gémeas semeadas por argumentos (ver mais abaixo).

const N: usize = 40_000;

fn src_vars_ifs(n: usize) -> String {
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("v{i} = {i}\n"));
    }
    for i in 0..n {
        src.push_str(&format!("if v{i} > 5:\n    v{i} = 1\n"));
    }
    src.push_str("print(v0)\n");
    src
}

fn src_elif_chain_else(n: usize) -> String {
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("v{i} = {i}\n"));
    }
    src.push_str("if v0 > 5:\n    z = 0\n");
    for i in 1..n {
        src.push_str(&format!("elif v{i} > 5:\n    z = {i}\n"));
    }
    src.push_str("else:\n    z = 99\nprint(z)\n");
    src
}

fn src_elif_chain_no_else(n: usize) -> String {
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("v{i} = {i}\n"));
    }
    src.push_str("if v0 > 5:\n    v0 = 1\n");
    for i in 1..n {
        src.push_str(&format!("elif v{i} > 5:\n    v{i} = 1\n"));
    }
    src.push_str("print(v0)\n");
    src
}

fn src_while_true_breaks(n: usize) -> String {
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
    src
}

// --- Pipeline completo (`cudrac check`: lexer + parser + sema + verificação
// da TIR) sobre as entradas adversárias. Ao contrário dos testes acima, correm
// em processo e impõem o limite também em debug — é o que o `scripts/check.sh`
// corre, onde o `cudrac` não está otimizado.

const PIPELINE_LIMIT: Duration = Duration::from_secs(2);

// A TIR representa um `elif` como um `if` aninhado; largar um módulo desses
// recursivamente precisa da pilha grande (a thread principal do `cargo test`
// tem 2 MiB e rebenta), por isso o resultado é largado cá dentro.
fn check_pipeline_fast(name: &str, src: &str) {
    let src = src.to_string();
    let (dt, err) = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let t0 = Instant::now();
            let err = match check_source("perf.cdr", &src, false) {
                Ok(tir) => {
                    drop(tir);
                    None
                }
                Err(e) => Some(format!("{e:?}")),
            };
            (t0.elapsed(), err)
        })
        .unwrap()
        .join()
        .unwrap();
    eprintln!("perf pipeline {name}: {dt:?}");
    if let Some(e) = err {
        panic!("{name}: o pipeline rejeitou uma entrada válida: {e}");
    }
    assert!(
        dt < PIPELINE_LIMIT,
        "{name} demorou {dt:?} (limite {PIPELINE_LIMIT:?})"
    );
}

#[test]
fn perf_40k_elif_chain_else() {
    check_pipeline_fast("40k_elif_chain_else", &src_elif_chain_else(N));
}

#[test]
fn perf_40k_elif_chain_no_else() {
    check_pipeline_fast("40k_elif_chain_no_else", &src_elif_chain_no_else(N));
}

#[test]
fn perf_40k_while_true_breaks() {
    check_pipeline_fast("40k_while_true_breaks", &src_while_true_breaks(N));
}

#[test]
fn perf_5000_vars_5000_ifs() {
    check_under_2s("5000_vars_5000_ifs", &src_vars_ifs(5000));
}

// --- Build completo (`cudrac build`: pipeline + codegen LLVM + objeto +
// ligação contra o runtime). É o caminho que o utilizador paga a compilar, e
// acrescenta ao `check` duas fases com custos próprios (o codegen percorre a
// TIR inteira — recursivamente, no caso dos `elif` aninhados — e o `--release`
// passa `default<O3>` pelo módulo; a ligação invoca o `cc`).
//
// O limite é folgado de propósito, ao contrário do `PIPELINE_LIMIT`: medido em
// ~0,7-1,4 s por caso e a correr em paralelo com o resto do `--workspace`, o
// que interessa aqui é travar o crescimento assintótico, não micro-variações.

const BUILD_LIMIT: Duration = Duration::from_secs(8);

// --- Entradas que o `default<O3>` não consegue apagar
//
// Com constantes, as formas adversárias de cima são apagadas inteiras no
// `--release`: no caso de 40k o `default<O3>` reduz o programa a 641 bytes de
// IR e o objeto fica com ~1 KB (contra 2,88 MB sem otimizações), portanto um
// teste de release mediria o custo de *eliminar* código, não o de o compilar.
// Aqui a semente vem sempre de `arg_int(1)` — o argumento do programa — e
// todos os locais continuam vivos até ao fim (a soma final impede o DCE), que
// é o que um programa real obriga o `--release` a pagar. Os testes abaixo
// correm sempre o executável, para que uma otimização a mais não passe por
// acidente.
//
// O que isto revelou (tempo de codegen + passes de máquina, sem ligação):
// em debug é linear (1000 → 0,09 s, 4000 → 0,5 s) e em `--release` é
// claramente superlinear, porque o `SROAPass` do `default<O3>` percorre a
// função uma vez por `alloca` e o nosso codegen emite uma `alloca` por local
// (confirmado com `opt -O3 -time-passes`: SROA é 80% do pipeline de módulo):
//
// | forma              | 1000  | 2000  | 4000  |
// |--------------------|-------|-------|-------|
// | vars_ifs           | 0,33  | 1,12  | 4,0   |
// | elif_chain_else    | 1,13  | 5,50  | 16,8  |
// | while_true_breaks  | 0,56  | 1,94  | 6,5   |
//
// Daí os tamanhos diferentes: em debug 4000 locais ainda cabem folgadamente no
// limite, em release só 1000 — 8000 locais em release já levam 24,7 s e 40k
// deixariam de ser compiláveis em tempo útil. Os dois casos maiores correm só
// a pedido (`--ignored`): 8000 em release e 40k em debug.

const DYN_DEBUG_N: usize = 4_000;
const DYN_RELEASE_N: usize = 1_000;

fn src_vars_ifs_dyn(n: usize) -> String {
    let mut src = String::from("from cudra import arg_int\nsemente = arg_int(1)\n");
    for i in 0..n {
        src.push_str(&format!("v{i} = semente + {i}\n"));
    }
    for i in 0..n {
        src.push_str(&format!("if v{i} > 5:\n    v{i} = v{i} - semente\n"));
    }
    src.push_str("soma = 0\n");
    for i in 0..n {
        src.push_str(&format!("soma = soma + v{i}\n"));
    }
    src.push_str("print(soma)\n");
    src
}

fn src_elif_chain_else_dyn(n: usize) -> String {
    let mut src = String::from("from cudra import arg_int\nsemente = arg_int(1)\n");
    for i in 0..n {
        src.push_str(&format!("v{i} = semente + {i}\n"));
    }
    src.push_str("if v0 > 5:\n    z = v0 - semente\n");
    for i in 1..n {
        src.push_str(&format!("elif v{i} > 5:\n    z = v{i} - semente\n"));
    }
    src.push_str("else:\n    z = 99\nprint(z)\n");
    src
}

fn src_while_true_breaks_dyn(n: usize) -> String {
    let mut src = String::from("from cudra import arg_int\nsemente = arg_int(1)\nsoma = 0\n");
    for i in 0..n {
        src.push_str(&format!("v{i} = semente + {i}\n"));
    }
    src.push_str("while True:\n");
    for i in 0..n {
        src.push_str(&format!(
            "    if v{i} > 5:\n        b{i} = v{i} - semente\n        soma = soma + b{i}\n        break\n"
        ));
    }
    src.push_str("print(soma)\n");
    src
}

// Com semente = 0: todos os `v{i}` ficam em `i` (todos com `i > 5` são
// subtraídos), logo a soma é `n*(n-1)/2`; a cadeia `elif` pára no `i = 6` e dá
// `z = 6`; o `while True` também sai no `i = 6`, com `soma = 6`.
fn soma_ate(n: usize) -> String {
    format!("{}\n", n * (n - 1) / 2)
}

// A TIR de um `elif` é um `if` aninhado: largar o resultado (e percorrê-lo no
// codegen) precisa da pilha grande, porque a thread do `cargo test` tem 2 MiB.
// Devolve o temporário, para o executável continuar em disco depois do build.
fn build_fast(
    name: &str,
    src: &str,
    release: bool,
    limit: Duration,
) -> (tempfile::TempDir, PathBuf) {
    let src = src.to_string();
    let name = name.to_string();
    std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(move || {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("perf.cdr");
            fs::write(&path, &src).unwrap();
            let exe = dir.path().join("perf");
            let t0 = Instant::now();
            let built = cudra_driver::build_executable(&path, &exe, release);
            let dt = t0.elapsed();
            eprintln!("perf build {name} (release={release}): {dt:?}");
            match built {
                Ok(()) => {
                    let len = fs::metadata(&exe).expect("o executável não existe").len();
                    assert!(len > 0, "{name}: o executável ficou vazio");
                }
                Err(f) => panic!("{name}: o build falhou: {f:?}"),
            }
            assert!(dt < limit, "{name} demorou {dt:?} (limite {limit:?})");
            (dir, exe)
        })
        .unwrap()
        .join()
        .unwrap()
}

// Depois de ligado, o executável é sempre corrido: um build que obedece ao
// limite mas produz um programa errado não serve de nada, e o `O3` mexe em
// código que o nível `None` nunca toca. Corre sempre com a semente `0`, que é
// a que `soma_ate` e os valores esperados assumem.
fn build_fast_and_run(name: &str, src: &str, release: bool, esperado: &[u8], limit: Duration) {
    let (_dir, exe) = build_fast(name, src, release, limit);
    let out = Command::new(&exe)
        .arg("0")
        .output()
        .expect("o executável não correu");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{name}: o executável saiu com {:?}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout, esperado,
        "{name}: saída inesperada do executável"
    );
}

#[test]
fn perf_build_dyn_vars_ifs() {
    build_fast_and_run(
        "dyn_vars_ifs",
        &src_vars_ifs_dyn(DYN_DEBUG_N),
        false,
        soma_ate(DYN_DEBUG_N).as_bytes(),
        BUILD_LIMIT,
    );
}

#[test]
fn perf_build_dyn_elif_chain_else() {
    build_fast_and_run(
        "dyn_elif_chain_else",
        &src_elif_chain_else_dyn(DYN_DEBUG_N),
        false,
        b"6\n",
        BUILD_LIMIT,
    );
}

#[test]
fn perf_build_dyn_while_true_breaks() {
    build_fast_and_run(
        "dyn_while_true_breaks",
        &src_while_true_breaks_dyn(DYN_DEBUG_N),
        false,
        b"6\n",
        BUILD_LIMIT,
    );
}

#[test]
fn perf_build_dyn_vars_ifs_release() {
    build_fast_and_run(
        "dyn_vars_ifs_release",
        &src_vars_ifs_dyn(DYN_RELEASE_N),
        true,
        soma_ate(DYN_RELEASE_N).as_bytes(),
        BUILD_LIMIT,
    );
}

#[test]
fn perf_build_dyn_elif_chain_else_release() {
    build_fast_and_run(
        "dyn_elif_chain_else_release",
        &src_elif_chain_else_dyn(DYN_RELEASE_N),
        true,
        b"6\n",
        BUILD_LIMIT,
    );
}

#[test]
fn perf_build_dyn_while_true_breaks_release() {
    build_fast_and_run(
        "dyn_while_true_breaks_release",
        &src_while_true_breaks_dyn(DYN_RELEASE_N),
        true,
        b"6\n",
        BUILD_LIMIT,
    );
}

// O limite aqui é uma rede contra uma regressão catastrófica, não um alvo: em
// `--release` o `SROAPass` já é superlinear no número de locais (ver a tabela
// acima) e mede-se entre ~30 s e ~90 s neste tamanho, conforme a carga do
// host. Corre-se a pedido:
//     cargo test -p cudra-driver --test perf -- --ignored --nocapture
const HEAVY_LIMIT: Duration = Duration::from_secs(300);

#[test]
#[ignore = "caro: mede a degeneração superlinear do SROA em --release"]
fn perf_build_8000_elif_chain_else_release() {
    build_fast_and_run(
        "8000_elif_chain_else_release",
        &src_elif_chain_else_dyn(8_000),
        true,
        b"6\n",
        HEAVY_LIMIT,
    );
}

// O mesmo tamanho das entradas apagáveis, mas semeadas: em debug o codegen é
// linear e não há `default<O3>`, por isso 40k locais ainda são pagáveis fora da
// suite normal.
#[test]
#[ignore = "caro: mede o codegen em debug com 40k locais semeados"]
fn perf_build_40k_vars_ifs_dyn() {
    build_fast_and_run(
        "40k_vars_ifs_dyn",
        &src_vars_ifs_dyn(N),
        false,
        soma_ate(N).as_bytes(),
        HEAVY_LIMIT,
    );
}
