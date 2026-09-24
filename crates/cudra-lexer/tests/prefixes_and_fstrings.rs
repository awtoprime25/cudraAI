mod common;
use common::lex_src;
use cudra_diagnostics::Diagnostic;
use cudra_lexer::TokenKind;

fn only_diag(diags: &[Diagnostic]) -> &Diagnostic {
    assert_eq!(
        diags.len(),
        1,
        "esperava exatamente 1 diagnóstico: {diags:?}"
    );
    &diags[0]
}

#[test]
fn invalid_two_letter_prefixes_lex_as_name_plus_str() {
    for bad in ["ub", "bu", "uR", "ur", "ff", "rr", "bf"] {
        let src = format!("x = {bad}'a'\n");
        let (tokens, diags) = lex_src(&src);
        assert!(
            diags.is_empty(),
            "'{bad}\\'a\\'' não devia dar erro: {diags:?}"
        );
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Name,
                TokenKind::Eq,
                TokenKind::Name,
                TokenKind::Str,
                TokenKind::Newline,
                TokenKind::Eof,
            ],
            "prefixo '{bad}' devia dar Name + Str separados"
        );
    }
}

#[test]
fn valid_two_letter_prefixes_in_any_case_lex_as_one_str() {
    for good in ["Rb", "fR", "Br", "rB"] {
        let src = format!("x = {good}'a'\n");
        let (tokens, diags) = lex_src(&src);
        assert!(
            diags.is_empty(),
            "'{good}\\'a\\'' não devia dar erro: {diags:?}"
        );
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Name,
                TokenKind::Eq,
                TokenKind::Str,
                TokenKind::Newline,
                TokenKind::Eof,
            ],
            "prefixo '{good}' devia dar um único Str"
        );
    }
}

fn assert_one_str_no_errors(src: &str) {
    let (tokens, diags) = lex_src(src);
    assert!(diags.is_empty(), "'{src}' não devia dar erro: {diags:?}");
    let str_count = tokens.iter().filter(|t| t.kind == TokenKind::Str).count();
    assert_eq!(
        str_count, 1,
        "'{src}' devia dar exatamente 1 Str: {tokens:?}"
    );
}

#[test]
fn fstring_with_same_quote_nested_string_is_one_token() {
    assert_one_str_no_errors("x = f'{d['k']}'\n");
}

#[test]
fn fstring_with_backslash_continuation_in_field_is_one_token() {
    assert_one_str_no_errors("x = f'{a +\\\n b}'\n");
}

#[test]
fn fstring_with_comment_in_field_is_one_token() {
    assert_one_str_no_errors("x = f'{a # c\n}'\n");
}

#[test]
fn fstring_with_format_spec_nested_field_is_one_token() {
    assert_one_str_no_errors("x = f'{a:>{w}}'\n");
}

#[test]
fn fstring_with_doubled_braces_is_one_token() {
    assert_one_str_no_errors("x = f'{{a}}'\n");
}

#[test]
fn fstring_with_mixed_quotes_in_field_is_one_token() {
    assert_one_str_no_errors("x = f\"{'x' if a else \"y\"}\"\n");
}

#[test]
fn triple_quoted_fstring_with_same_triple_quote_nested_is_one_token() {
    assert_one_str_no_errors("x = f'''{'''a'''}'''\n");
}

#[test]
fn fstring_with_nested_fstring_field_is_one_token() {
    assert_one_str_no_errors("x = f'{f'{a}'}'\n");
}

#[test]
fn raw_fstring_backslash_in_literal_is_one_token() {
    assert_one_str_no_errors("x = rf'\\d{a}'\n");
}

#[test]
fn fstring_with_conversion_is_one_token() {
    assert_one_str_no_errors("x = f'{a!r}'\n");
}

#[test]
fn fstring_with_debug_spec_is_one_token() {
    assert_one_str_no_errors("x = f'{a=}'\n");
}

#[test]
fn fstring_field_close_followed_by_literal_braces_is_one_token() {
    assert_one_str_no_errors("x = f'{a}}}'\n");
}

#[test]
fn uppercase_f_prefix_with_backslash_continuation_in_parens_is_one_token() {
    assert_one_str_no_errors("x = F'{(a,\\\n b)}'\n");
}

#[test]
fn fstring_with_multiple_fields_and_nested_format_specs_is_one_token() {
    assert_one_str_no_errors("x = f'x{a}y{b:{c}.{d}}z'\n");
}

#[test]
fn fstring_lone_closing_brace_in_literal_is_rejected() {
    let src = "x = f'a}b'\n";
    let (_, diags) = lex_src(src);
    assert!(
        diags.iter().any(|d| d.message.contains("'}' sozinho")),
        "esperava erro de '}}' sozinho: {diags:?}"
    );
}

#[test]
fn fstring_unclosed_field_at_eof_is_rejected() {
    let src = "x = f'{a'";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "string sem fecho");
}

#[test]
fn fstring_unclosed_field_with_newline_at_eof_does_not_panic() {
    let src = "x = f'{a\n'";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "string sem fecho");
}

#[test]
fn backslash_before_open_brace_does_not_consume_it_and_field_opens() {
    assert_one_str_no_errors("x = f'\\{a}'\n");
}

#[test]
fn raw_fstring_backslash_before_open_brace_does_not_consume_it_and_field_opens() {
    assert_one_str_no_errors("x = rf'\\{a}'\n");
}

#[test]
fn backslash_before_close_brace_does_not_consume_it_then_doubled_escape() {
    // '\}}': o '\' não consome o primeiro '}', que depois forma '}}' com o
    // seguinte (escape literal de '}').
    assert_one_str_no_errors("x = f'\\}}'\n");
}

#[test]
fn backslash_before_open_brace_then_doubled_escape() {
    assert_one_str_no_errors("x = f'\\{{'\n");
}

#[test]
fn named_escape_followed_by_field_is_one_token() {
    assert_one_str_no_errors("x = f'\\N{EM DASH}{a}'\n");
}

#[test]
fn named_escape_inside_format_spec_is_one_token() {
    assert_one_str_no_errors("x = f'{a:\\N{BULLET}}'\n");
}

#[test]
fn named_escape_unterminated_before_closing_quote_has_no_lexical_error() {
    // '\N{' e '\N{EM DASH' sem '}': o escape pára ANTES da aspa de fecho
    // sem a consumir, por isso a string fecha normalmente — o compile()
    // rejeita-os por erro de unicode, o que não é um problema léxico nosso.
    assert_one_str_no_errors("x = f'\\N{'\n");
    assert_one_str_no_errors("x = f'\\N{EM DASH'\n");
}

#[test]
fn named_escape_followed_by_lone_closing_brace_is_rejected() {
    let src = "x = f'\\N{EM DASH}}'\n";
    let (_, diags) = lex_src(src);
    assert!(
        diags.iter().any(|d| d.message.contains("'}' sozinho")),
        "esperava erro de '}}' sozinho: {diags:?}"
    );
}

#[test]
fn raw_fstring_named_escape_is_not_special_and_brace_opens_a_field() {
    // numa f-string raw, '\N' é só '\' + 'N' — o '{' a seguir abre um
    // campo real.
    assert_one_str_no_errors("x = rf'\\N{x}'\n");
}

#[test]
fn backslash_not_followed_by_newline_inside_field_is_rejected_and_recovers() {
    let src = "x = f'{\\n}'\n";
    let (tokens, diags) = lex_src(src);
    assert!(
        diags
            .iter()
            .any(|d| d.message == "carácter inesperado '\\'"),
        "esperava erro de carácter inesperado: {diags:?}"
    );
    // a lexagem recupera: a string ainda fecha como 1 só Str.
    let str_count = tokens.iter().filter(|t| t.kind == TokenKind::Str).count();
    assert_eq!(str_count, 1);
}
