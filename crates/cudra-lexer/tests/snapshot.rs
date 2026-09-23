mod common;
use common::dump;

macro_rules! snap {
    ($name:ident, $src:expr) => {
        #[test]
        fn $name() {
            insta::assert_snapshot!(dump($src));
        }
    };
}

snap!(basic_assign, "x = 1\n");

snap!(
    if_elif_else,
    "if x:\n    y = 1\nelif w:\n    pass\nelse:\n    y = 2\n"
);

snap!(
    nested_for_multi_dedent_eof,
    "for i in range(3):\n    for j in range(3):\n        x = i + j"
);

snap!(while_loop, "i = 0\nwhile i < 10:\n    i += 1\n");

snap!(
    def_with_params_and_return,
    "def f(a: int, b: float) -> float:\n    return a + b\n"
);

snap!(
    comments_and_blank_lines,
    "x = 1\n\n# comentario solto\n\ny = 2  # comentario no fim\n"
);

snap!(one_line_block, "if x: y = 1\n");

snap!(
    bracket_continuation_with_comment,
    "x = (1,\n  # comentario\n  2,\n  3)\n"
);

snap!(backslash_continuation, "x = 1 + \\\n    2\n");

snap!(crlf_line_endings, "x = 1\r\ny = 2\r\n");

snap!(
    number_literals_int,
    "a = 0\nb = 00\nc = 0x_ff\nd = 0b_1\ne = 0o17\nf = 1_000\n"
);

snap!(
    number_literals_float,
    "a = 1.5\nb = 1.\nc = .5\nd = 1e10\ne = 1E-3\nf = 1_0.5e-3\n"
);

snap!(
    string_literals_simple,
    "a = 'abc'\nb = \"abc\"\nc = 'ele\\'fante'\n"
);

snap!(
    string_literals_triple_and_prefix,
    "a = '''linha1\nlinha2'''\nb = rb'raw bytes'\nc = f\"fstr\"\n"
);

snap!(
    import_and_from_import,
    "import cudra\nfrom cudra import arg_int, arg_float\n"
);

snap!(
    all_augmented_assign_ops,
    "x += 1\nx -= 1\nx *= 1\nx /= 1\nx //= 1\nx %= 1\nx **= 1\nx &= 1\nx |= 1\nx ^= 1\nx <<= 1\nx >>= 1\n"
);

snap!(comparisons_chained, "z = a < b <= c == d != e > f >= g\n");

snap!(bitwise_and_shift_ops, "z = a & b | c ^ d << e >> ~f\n");

snap!(
    reserved_keywords,
    "class x:\n    pass\nlambda_ = 1\ntry_ = 1\n"
);

snap!(true_false_none, "a = True\nb = False\nc = None\n");

snap!(empty_file, "");

snap!(only_comments_and_blank_lines, "# so isto\n\n   \n# mais\n");

snap!(
    deep_dedent_no_trailing_newline,
    "if a:\n    if b:\n        if c:\n            x = 1"
);

snap!(tab_in_indentation_recovers, "if x:\n\ty = 1\n    z = 2\n");

snap!(inconsistent_dedent_recovers, "if x:\n   y = 1\n  z = 2\n");

snap!(
    numeric_literal_errors,
    "a = 1__0\nb = 10_\nc = 0x\nd = 1e\ne = 0777\nf = 1j\ng = 1if\n"
);

snap!(
    unexpected_characters_recover,
    "a = 1 ! 2\nb = $\nc = ?\nd = `\n"
);

snap!(unterminated_string_recovers, "x = 'abc\ny = 1\n");

snap!(non_ascii_identifier_error, "café = 1\n");

snap!(unmatched_closing_bracket_recovers, "x = 1)\ny = [2\n");

snap!(first_line_indented_is_error, "    x = 1\n");
