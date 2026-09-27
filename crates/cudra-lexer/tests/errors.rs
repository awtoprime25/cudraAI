mod common;
use common::lex_src;
use cudra_diagnostics::Diagnostic;
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

#[test]
fn tabs_in_indentation_reports_exact_span() {
    let src = "if x:\n\ty = 1\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(
        d.message,
        "tabs na indentação ainda não são suportados na Fase 1; usa espaços"
    );
    let tab = src.find('\t').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), tab, tab + 1));
}

#[test]
fn inconsistent_dedent_reports_exact_span() {
    let src = "if x:\n   y = 1\n  z = 2\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "indentação inconsistente");
    let z = src.find('z').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), z, z));
}

#[test]
fn unexpected_indent_on_first_logical_line() {
    let src = "    x = 1\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "indentação inesperada");
    assert_eq!(primary_span(d), Span::new(FileId(0), 0, 4));
}

#[test]
fn unexpected_indent_skips_leading_blank_and_comment_lines() {
    // linhas em branco/comentário antes da primeira linha lógica não contam.
    let src = "\n# c\n\n  x = 1\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "indentação inesperada");
    let lo = src.find("  x").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 2));
}

#[test]
fn leading_zero_decimal_int_is_rejected() {
    let src = "x = 0777\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(
        d.message,
        "zeros à esquerda em literais decimais não são permitidos; usa 0o para octal"
    );
    let lo = src.find("0777").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 4));
}

#[test]
fn all_zero_decimal_is_accepted() {
    let (tokens, diags) = lex_src("x = 00\n");
    assert!(diags.is_empty(), "'00' não devia dar erro: {diags:?}");
    assert_eq!(tokens[2].kind, cudra_lexer::TokenKind::Int);
}

#[test]
fn double_underscore_in_int_is_rejected() {
    let src = "x = 1__0\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "sublinhado inválido em literal numérico");
    let lo = src.find("1__0").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 4));
}

#[test]
fn trailing_underscore_in_int_is_rejected() {
    let src = "x = 10_\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "sublinhado inválido em literal numérico");
    let lo = src.find("10_").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 3));
}

#[test]
fn underscore_right_after_decimal_point_is_rejected() {
    let src = "x = 1._5\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "sublinhado inválido em literal numérico");
    let lo = src.find("1._5").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 4));
}

#[test]
fn incomplete_exponent_is_rejected() {
    let src = "x = 1e\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "número inválido: dígitos em falta");
    let lo = src.find("1e").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 2));
}

#[test]
fn incomplete_exponent_with_sign_is_rejected() {
    let src = "x = 1e+\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "número inválido: dígitos em falta");
    let lo = src.find("1e+").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 3));
}

#[test]
fn incomplete_hex_literal_is_rejected() {
    let src = "x = 0x\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "literal hexadecimal inválido: dígitos em falta");
    let lo = src.find("0x").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 2));
}

#[test]
fn incomplete_octal_literal_is_rejected() {
    let src = "x = 0o\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "literal octal inválido: dígitos em falta");
    let lo = src.find("0o").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 2));
}

#[test]
fn incomplete_binary_literal_is_rejected() {
    let src = "x = 0b\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "literal binário inválido: dígitos em falta");
    let lo = src.find("0b").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 2));
}

#[test]
fn hex_with_leading_underscore_after_prefix_is_accepted() {
    let (tokens, diags) = lex_src("x = 0x_ff\n");
    assert!(diags.is_empty(), "'0x_ff' não devia dar erro: {diags:?}");
    assert_eq!(tokens[2].kind, cudra_lexer::TokenKind::Int);
}

#[test]
fn invalid_digit_in_binary_literal_is_rejected() {
    let src = "x = 0b10106\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "dígito inválido '6' em literal binário");
    let lo = src.find('6').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn invalid_digit_in_octal_literal_is_rejected() {
    let src = "x = 0o178\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "dígito inválido '8' em literal octal");
    let lo = src.find('8').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn invalid_first_digit_in_octal_literal_is_rejected() {
    let src = "x = 0o8\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "dígito inválido '8' em literal octal");
    let lo = src.find('8').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn invalid_first_digit_in_binary_literal_is_rejected() {
    let src = "x = 0b2\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "dígito inválido '2' em literal binário");
    let lo = src.find('2').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn glued_letters_in_binary_literal_is_rejected() {
    let src = "x = 0b1a\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "literal binário inválido");
    let lo = src.find("0b1a").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 4));
}

#[test]
fn glued_letters_in_octal_literal_is_rejected() {
    let src = "x = 0o1z\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "literal octal inválido");
    let lo = src.find("0o1z").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 4));
}

#[test]
fn glued_letters_in_hex_literal_with_no_valid_digits_is_rejected() {
    let src = "x = 0xg\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "literal hexadecimal inválido");
    let lo = src.find("0xg").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 3));
}

#[test]
fn hex_digit_out_of_range_letter_is_not_glued_into_literal() {
    // '0x1g': 'g' não é dígito hex; o literal inteiro é inválido (não é
    // "falta um espaço", que seria enganador aqui).
    let src = "x = 0x1g\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "literal hexadecimal inválido");
    let lo = src.find("0x1g").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 4));
}

#[test]
fn complex_int_suffix_is_rejected() {
    let src = "x = 1j\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(
        d.message,
        "números complexos ainda não são suportados na Fase 1"
    );
    let lo = src.find("1j").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 2));
}

#[test]
fn complex_float_suffix_is_rejected() {
    let src = "x = 1.5J\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(
        d.message,
        "números complexos ainda não são suportados na Fase 1"
    );
    let lo = src.find("1.5J").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 4));
}

#[test]
fn number_glued_to_name_is_rejected() {
    let src = "x = 1if\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "falta um espaço entre o número e 'if'");
    let lo = src.find("1if").unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 3));
}

#[test]
fn non_ascii_identifier_is_one_error_covering_the_whole_name() {
    let src = "café = 1\n";
    let (tokens, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(
        d.message,
        "identificadores não-ASCII ainda não são suportados na Fase 1"
    );
    let lo = src.find("café").unwrap() as u32;
    let hi = lo + "café".len() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, hi));
    // não há nenhum token "caf": só o Eq/Int/Newline/Eof do resto da linha.
    assert!(tokens
        .iter()
        .all(|t| t.kind != cudra_lexer::TokenKind::Name));
}

#[test]
fn unexpected_bang_alone_is_rejected() {
    let src = "x = 1 ! 2\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "carácter inesperado '!'");
    let lo = src.find('!').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn not_eq_is_not_flagged_as_unexpected() {
    let (_, diags) = lex_src("x = a != b\n");
    assert!(diags.is_empty(), "'!=' não devia dar erro: {diags:?}");
}

#[test]
fn unexpected_dollar_is_rejected() {
    let src = "x = $\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "carácter inesperado '$'");
    let lo = src.find('$').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn unexpected_control_byte_uses_hex_code() {
    let src = "x = \u{1}\n";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "carácter inesperado '\\x01'");
    let lo = src.find('\u{1}').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
}

#[test]
fn unterminated_single_line_string_recovers_at_newline() {
    let src = "x = 'abc\ny = 1\n";
    let (tokens, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "string sem fecho");
    let lo = src.find('\'').unwrap() as u32;
    let hi = src.find('\n').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, hi));
    // a lexagem recupera: a segunda linha continua a ser tokenizada.
    assert!(tokens
        .iter()
        .any(|t| t.kind == cudra_lexer::TokenKind::Name));
}

#[test]
fn unterminated_triple_string_recovers_at_eof() {
    let src = "x = '''abc\ndef";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "string sem fecho");
    let lo = src.find("'''").unwrap() as u32;
    let hi = src.len() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, hi));
}

#[test]
fn unmatched_rparen_reports_and_still_emits_token() {
    let src = "x = 1)\n";
    let (tokens, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "')' sem abertura");
    let lo = src.find(')').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
    assert!(tokens
        .iter()
        .any(|t| t.kind == cudra_lexer::TokenKind::RParen));
}

#[test]
fn unmatched_rbracket_reports_and_still_emits_token() {
    let src = "x = 1]\n";
    let (tokens, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "']' sem abertura");
    let lo = src.find(']').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
    assert!(tokens
        .iter()
        .any(|t| t.kind == cudra_lexer::TokenKind::RBracket));
}

#[test]
fn indent_and_dedent_counts_always_match_even_with_errors() {
    let src = "if x:\n\ty = 1\n    if z:\n\tw = 2\n";
    let (tokens, _) = lex_src(src);
    let indents = tokens
        .iter()
        .filter(|t| t.kind == cudra_lexer::TokenKind::Indent)
        .count();
    let dedents = tokens
        .iter()
        .filter(|t| t.kind == cudra_lexer::TokenKind::Dedent)
        .count();
    assert_eq!(indents, dedents);
}

#[test]
fn null_byte_in_comment_reports_exactly_one_error() {
    let src = "# a\0b\nx = 1\n";
    let (tokens, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "o ficheiro contém bytes nulos (0x00)");
    let lo = src.find('\0').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
    // o resto da lexagem continua normalmente.
    assert!(tokens
        .iter()
        .any(|t| t.kind == cudra_lexer::TokenKind::Name));
}

#[test]
fn null_byte_in_string_reports_exactly_one_error() {
    let src = "x = 'a\0b'\n";
    let (tokens, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "o ficheiro contém bytes nulos (0x00)");
    let lo = src.find('\0').unwrap() as u32;
    assert_eq!(primary_span(d), Span::new(FileId(0), lo, lo + 1));
    assert!(tokens.iter().any(|t| t.kind == cudra_lexer::TokenKind::Str));
}

#[test]
fn null_byte_alone_reports_exactly_one_error() {
    let src = "\0";
    let (_, diags) = lex_src(src);
    let d = only_diag(&diags);
    assert_eq!(d.message, "o ficheiro contém bytes nulos (0x00)");
    assert_eq!(primary_span(d), Span::new(FileId(0), 0, 1));
}
