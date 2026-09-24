mod common;
use common::lex_src;
use cudra_lexer::TokenKind;

// --- BUG L7: tokens novos (maximal munch, como o CPython) ------------------

#[test]
fn at_eq_is_one_token() {
    let (tokens, diags) = lex_src("x @= y\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::AtEq,
            TokenKind::Name,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn colon_eq_is_one_token() {
    let (tokens, diags) = lex_src("if (x := f()):\n    pass\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    assert!(tokens.iter().any(|t| t.kind == TokenKind::ColonEq));
}

#[test]
fn ellipsis_is_one_token() {
    let (tokens, diags) = lex_src("x = ...\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Ellipsis,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn semi_is_one_token() {
    let (tokens, diags) = lex_src("x = 1; y = 2\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    assert!(tokens.iter().any(|t| t.kind == TokenKind::Semi));
}

#[test]
fn braces_are_one_token_each_and_open_close_cleanly() {
    let (tokens, diags) = lex_src("x = {1: 2}\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::LBrace,
            TokenKind::Int,
            TokenKind::Colon,
            TokenKind::Int,
            TokenKind::RBrace,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn unclosed_brace_at_eof_is_rejected() {
    let src = "x = {1: 2\n";
    let (_, diags) = lex_src(src);
    assert_eq!(diags.len(), 1, "esperava exatamente 1 erro: {diags:?}");
    assert_eq!(diags[0].message, "'{' nunca foi fechado");
}

#[test]
fn brace_participates_in_implicit_line_joining() {
    let src = "x = {\n    1: 2,\n}\n";
    let (_, diags) = lex_src(src);
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let (tokens, _) = lex_src(src);
    // dentro de '{...}' as quebras de linha não geram Newline/Indent/Dedent.
    assert!(tokens
        .iter()
        .all(|t| t.kind != TokenKind::Indent && t.kind != TokenKind::Dedent));
}

// --- L7: sequências de '.' (maximal munch) ----------------------------------

#[test]
fn double_dot_is_two_dot_tokens() {
    let (tokens, diags) = lex_src("x = a..b\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Name,
            TokenKind::Dot,
            TokenKind::Dot,
            TokenKind::Name,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn quadruple_dot_is_ellipsis_then_dot() {
    let (tokens, diags) = lex_src("x = a....b\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Name,
            TokenKind::Ellipsis,
            TokenKind::Dot,
            TokenKind::Name,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn triple_dot_then_digit_is_ellipsis_then_int() {
    let (tokens, diags) = lex_src("x = ...5\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Ellipsis,
            TokenKind::Int,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn double_dot_then_digit_is_dot_then_float() {
    // '..5': o primeiro '.' fica sozinho (não é dígito a seguir), o segundo
    // '.5' é um float.
    let (tokens, diags) = lex_src("x = ..5\n");
    assert!(diags.is_empty(), "não devia dar erro: {diags:?}");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::Name,
            TokenKind::Eq,
            TokenKind::Dot,
            TokenKind::Float,
            TokenKind::Newline,
            TokenKind::Eof,
        ]
    );
}
