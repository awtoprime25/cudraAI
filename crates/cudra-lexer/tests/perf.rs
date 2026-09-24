use std::time::Instant;

use cudra_span::FileId;

#[test]
fn lexes_100k_lines_in_under_one_second_debug() {
    let mut src = String::with_capacity(100_000 * 6);
    for _ in 0..100_000 {
        src.push_str("x = 1\n");
    }

    let start = Instant::now();
    let (tokens, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();

    assert!(diags.is_empty(), "não devia haver erros: {diags:?}");
    assert_eq!(tokens.len(), 100_000 * 4 + 1); // Name,Eq,Int,Newline por linha + Eof
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "100k linhas demoraram {elapsed:?} (limite: 1s em debug)"
    );
}

// BUG L8: CPython limita a indentação a MAXINDENT=100 (99 níveis reais,
// medido empiricamente). Acima disso, 500 níveis não devem provocar stack
// overflow nem pânico — só 1 erro de limite, e a contagem de Indent/Dedent
// continua consistente (pára nos 99 níveis que couberam).
#[test]
fn handles_500_indentation_levels_without_stack_overflow() {
    let depth = 500;
    let mut src = String::new();
    for i in 0..depth {
        src.push_str(&"    ".repeat(i));
        src.push_str("if x:\n");
    }
    src.push_str(&"    ".repeat(depth));
    src.push_str("y = 1\n");

    let (tokens, diags) = cudra_lexer::lex(FileId(0), &src);
    assert_eq!(diags.len(), 1, "esperava exatamente 1 erro: {diags:?}");
    assert_eq!(
        diags[0].message,
        "demasiados níveis de indentação (máximo 99)"
    );

    let indents = tokens
        .iter()
        .filter(|t| t.kind == cudra_lexer::TokenKind::Indent)
        .count();
    let dedents = tokens
        .iter()
        .filter(|t| t.kind == cudra_lexer::TokenKind::Dedent)
        .count();
    assert_eq!(indents, 99);
    assert_eq!(dedents, 99);
}

// BUG L8: 1000 níveis de indentação de 1 espaço só cabem 99 vezes; o
// excesso dá exatamente 1 erro (não um por cada nível a mais).
#[test]
fn handles_1000_levels_of_one_space_indentation_with_a_single_limit_error() {
    let depth = 1000;
    let mut src = String::new();
    for i in 0..depth {
        src.push_str(&" ".repeat(i));
        src.push_str("if x:\n");
    }
    src.push_str(&" ".repeat(depth));
    src.push_str("y = 1\n");

    let start = Instant::now();
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();

    assert_eq!(diags.len(), 1, "esperava exatamente 1 erro: {diags:?}");
    assert_eq!(
        diags[0].message,
        "demasiados níveis de indentação (máximo 99)"
    );
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "1000 níveis de indentação demoraram {elapsed:?}"
    );
}

// BUG L8: 100 000 '(' consecutivos não devem provocar pânico nem
// recursão descontrolada — devem terminar depressa, com exatamente 1 erro
// de limite (mais o "nunca foi fechado" do parêntese mais exterior).
#[test]
fn handles_100k_consecutive_open_parens_without_panic() {
    let n = 100_000;
    let src = "(".repeat(n);

    let start = Instant::now();
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();

    assert_eq!(diags.len(), 2, "esperava 2 erros: {diags:?}");
    assert!(diags
        .iter()
        .any(|d| d.message == "demasiados parênteses aninhados (máximo 200)"));
    assert!(diags.iter().any(|d| d.message == "'(' nunca foi fechado"));
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "100k '(' consecutivos demoraram {elapsed:?}"
    );
}

#[test]
fn handles_a_single_huge_line_without_quadratic_blowup() {
    let terms = 50_000;
    let mut src = String::from("x = 1");
    for _ in 0..terms {
        src.push_str(" + 1");
    }
    src.push('\n');

    let start = Instant::now();
    let (tokens, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();

    assert!(diags.is_empty(), "não devia haver erros: {diags:?}");
    // Name, Eq, Int, depois (Plus, Int) * terms, Newline, Eof.
    assert_eq!(tokens.len(), 3 + terms * 2 + 2);
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "linha única com {terms} termos demorou {elapsed:?}"
    );
}

// BUG L5: o varrimento de f-strings tem de ser linear no nº de campos, não
// quadrático (nem recursivo sem limite) — 100k campos bem abaixo de 1s.
#[test]
fn fstring_with_100k_fields_lexes_in_under_one_second_debug() {
    let fields = 100_000;
    let mut src = String::from("x = f'");
    for _ in 0..fields {
        src.push_str("{a}");
    }
    src.push_str("'\n");

    let start = Instant::now();
    let (tokens, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();

    assert!(diags.is_empty(), "não devia haver erros: {diags:?}");
    let str_count = tokens
        .iter()
        .filter(|t| t.kind == cudra_lexer::TokenKind::Str)
        .count();
    assert_eq!(str_count, 1);
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "f-string com {fields} campos demorou {elapsed:?}"
    );
}

#[test]
fn lexes_100k_tokens_of_varied_content_in_under_one_second_debug() {
    let lines = 25_000;
    let mut src = String::new();
    for i in 0..lines {
        src.push_str(&format!("x{i} = a + b * (c - 1) - 2\n"));
    }

    let start = Instant::now();
    let (tokens, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();

    assert!(diags.is_empty(), "não devia haver erros: {diags:?}");
    assert!(tokens.len() >= 100_000, "só {} tokens", tokens.len());
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "100k tokens variados demoraram {elapsed:?}"
    );
}
