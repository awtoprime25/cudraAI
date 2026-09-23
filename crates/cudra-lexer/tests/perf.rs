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
    assert!(diags.is_empty(), "não devia haver erros: {diags:?}");

    let indents = tokens
        .iter()
        .filter(|t| t.kind == cudra_lexer::TokenKind::Indent)
        .count();
    let dedents = tokens
        .iter()
        .filter(|t| t.kind == cudra_lexer::TokenKind::Dedent)
        .count();
    assert_eq!(indents, depth);
    assert_eq!(dedents, depth);
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
