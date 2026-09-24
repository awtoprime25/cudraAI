mod common;
use common::lex_src;
use cudra_diagnostics::Diagnostic;
use cudra_lexer::TokenKind;
use cudra_span::{FileId, Span};

fn only_diag(diags: &[Diagnostic]) -> &Diagnostic {
    assert_eq!(
        diags.len(),
        1,
        "esperava exatamente 1 diagnóstico: {diags:?}"
    );
    &diags[0]
}

fn primary_span(diag: &Diagnostic) -> Span {
    diag.labels
        .iter()
        .find(|l| l.primary)
        .expect("diagnóstico sem label primário")
        .span
}

// --- BUG L1: linha física só com espaços + '\' começa a linha lógica -----

#[test]
fn backslash_only_line_after_blank_still_emits_newline() {
    let src = "\n\\\nx = 1\n";
    let (tokens, diags) = lex_src(src);
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Int,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn indented_backslash_only_line_emits_indent_before_content() {
    let src = "x += None\n \\\nx = 2\n";
    let (tokens, diags) = lex_src(src);
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::PlusEq,
            TokenKind::NoneKw,
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Int,
            TokenKind::Newline,
            TokenKind::Dedent,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn backslash_only_line_indentation_can_be_inconsistent() {
    let src = "if a:\n        if b:\n            c = 1\n    \\\nd = 2\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "indentação inconsistente");
}

#[test]
fn unindented_backslash_only_line_dedents_before_content() {
    let src = "if a:\n  b = 1\n\\\nc = 2\n";
    let (tokens, diags) = lex_src(src);
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::If,
            TokenKind::Name,
            TokenKind::Colon,
            TokenKind::Newline,
            TokenKind::Indent,
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Int,
            TokenKind::Newline,
            TokenKind::Dedent,
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Int,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

// --- BUG L3: continuação '\' seguida do fim do ficheiro -------------------

#[test]
fn backslash_then_newline_then_eof_is_rejected() {
    let src = "x = 1 \\\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "continuação com '\\' no fim do ficheiro");
    let lo = src.find('\\').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn backslash_with_no_trailing_newline_at_eof_is_rejected() {
    let src = "x = 1 \\";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "continuação com '\\' no fim do ficheiro");
    let lo = src.find('\\').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn backslash_only_line_then_eof_is_rejected() {
    let src = "if a: b = 1\n \\\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "continuação com '\\' no fim do ficheiro");
    let lo = src.rfind('\\').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

// --- BUG L2: parênteses/colchetes nunca fechados até ao EOF ---------------

#[test]
fn unclosed_paren_at_eof_is_rejected_at_the_opening_paren() {
    let src = "x = (1 + 2\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "'(' nunca foi fechado");
    let lo = src.find('(').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn unclosed_nested_brackets_report_only_the_outermost() {
    let src = "x = (1, [2, 3\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "'(' nunca foi fechado");
    let lo = src.find('(').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn closed_brackets_report_nothing() {
    let (_, diags) = lex_src("x = (1, [2, 3])\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
}
