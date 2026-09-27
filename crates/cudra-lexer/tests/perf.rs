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
    assert!(diags.iter().any(|d| d.message == "'(' sem fecho"));
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
fn one_million_open_parens_lexes_in_under_one_second_debug() {
    let src = "(".repeat(1_000_000);
    let start = Instant::now();
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();
    assert!(
        !diags.is_empty(),
        "esperava erros (limite L8 + '(' sem fecho)"
    );
    assert!(elapsed.as_secs_f64() < 1.0, "1M '(' demoraram {elapsed:?}");
}

#[test]
fn five_hundred_k_a_plus_lexes_in_under_one_second_debug() {
    let src = "a+".repeat(500_000);
    let start = Instant::now();
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();
    assert!(
        diags.is_empty(),
        "não devia haver erros: {:?}",
        &diags[..diags.len().min(3)]
    );
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "'a+' x 500k demoraram {elapsed:?}"
    );
}

#[test]
fn one_million_dollar_signs_lexes_in_under_one_second_debug() {
    let src = "$".repeat(1_000_000);
    let start = Instant::now();
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();
    assert_eq!(
        diags.len(),
        1,
        "corrida de '$' devia dar 1 só erro: {diags:?}"
    );
    assert!(elapsed.as_secs_f64() < 1.0, "1M '$' demoraram {elapsed:?}");
}

#[test]
fn run_of_unexpected_chars_without_whitespace_is_one_diagnostic() {
    let src = "x = $$$ 1\n";
    let (_, diags) = cudra_lexer::lex(FileId(0), src);
    assert_eq!(diags.len(), 1, "'$$$' devia dar 1 só erro: {diags:?}");
    assert!(diags[0].message.contains('$'));
}

#[test]
fn unexpected_chars_separated_by_space_are_separate_diagnostics() {
    let src = "x = $ $ $\n";
    let (_, diags) = cudra_lexer::lex(FileId(0), src);
    assert_eq!(
        diags.len(),
        3,
        "'$'s separados por espaço: 1 erro cada: {diags:?}"
    );
}

#[test]
fn lexer_stops_accumulating_diagnostics_after_the_limit() {
    let src = "$".repeat(2000) + " " + &"$ ".repeat(2000);
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    assert_eq!(
        diags.len(),
        1000,
        "esperava exatamente 1000 diagnósticos: {}",
        diags.len()
    );
    assert_eq!(
        diags.last().unwrap().message,
        "demasiados erros léxicos (1000); os restantes foram omitidos"
    );
}

#[test]
fn three_hundred_k_mismatched_paren_bracket_pairs_lexes_in_under_one_second_debug() {
    let src = "(]".repeat(300_000);
    let start = Instant::now();
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();
    assert_eq!(
        diags.len(),
        1000,
        "esperava o limite global de 1000 diagnósticos"
    );
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "'(]' x 300k demoraram {elapsed:?}"
    );
}

#[test]
fn one_million_dots_lexes_in_under_one_second_debug() {
    let src = ".".repeat(1_000_000);
    let start = Instant::now();
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();
    assert!(diags.is_empty(), "não devia haver erros: {diags:?}");
    assert!(elapsed.as_secs_f64() < 1.0, "1M '.' demoraram {elapsed:?}");
}

#[test]
fn one_million_null_bytes_lexes_in_under_one_second_debug() {
    let src = "\0".repeat(1_000_000);
    let start = Instant::now();
    let (_, diags) = cudra_lexer::lex(FileId(0), &src);
    let elapsed = start.elapsed();
    assert_eq!(diags.len(), 1, "esperava exatamente 1 erro: {diags:?}");
    assert_eq!(diags[0].message, "o ficheiro contém bytes nulos (0x00)");
    assert!(elapsed.as_secs_f64() < 1.0, "1M NUL demoraram {elapsed:?}");
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
