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

// --- precedência -------------------------------------------------------

snap!(precedence_neg_pow, "x = -2**2\n");
snap!(precedence_chained_compare, "x = a < b < c\n");
snap!(precedence_not_compare, "x = not a == b\n");
snap!(precedence_pow_right_assoc, "x = a**-b**c\n");
snap!(precedence_neg_pow_neg, "x = -a ** -b\n");
snap!(precedence_paren_compare, "x = (a < b) < c\n");
snap!(precedence_compare_paren, "x = a < (b < c)\n");
snap!(precedence_and_or_mixed, "x = a and b or c and d\n");
snap!(precedence_or_flatten_paren, "x = a or (b or c)\n");
snap!(precedence_and_flatten_paren, "x = (a and b) and c\n");
snap!(precedence_attr_on_invert, "x = ~a.b\n");
snap!(precedence_int_attr, "x = (1).real\n");
snap!(precedence_call_on_paren, "x = (a)(b)\n");
snap!(precedence_call_chain, "x = a.b.c(1)(2)[3]\n");
snap!(precedence_index_chain, "x = a[b][c]\n");
snap!(
    precedence_bitwise_shift,
    "x = a|b^c&d<<e>>f+g-h*i/j//k%l**m\n"
);

// --- blocos numa linha / ';' ---------------------------------------------

snap!(inline_if_semi, "if a: x = 1; y = 2\n");
snap!(while_break_continue_inline, "while a: break; continue\n");
snap!(trailing_semi, "x = 1;\n");
snap!(two_stmts_semi, "x = 1; y = 2\n");

// --- elif/else -----------------------------------------------------------

snap!(if_elif_else, "if a: pass\nelif b: pass\nelse: pass\n");
snap!(
    if_elif_elif_else,
    "if a:\n    x = 1\nelif b:\n    x = 2\nelif c:\n    x = 3\nelse:\n    x = 4\n"
);
snap!(if_only, "if a:\n    x = 1\n");

// --- decoradores / funções -------------------------------------------------

snap!(decorator_call_chain, "@a.b(1)(2)\n@c\ndef f(): pass\n");
snap!(func_arrow_none, "def f() -> None:\n    return\n");
snap!(
    func_array_generic_params,
    "def f(a: Array[Array[f32]],) -> Array[int,]:\n    pass\n"
);
snap!(
    func_typed_params,
    "def f(a: int, b: float) -> float:\n    return a / b\n"
);

// --- construções fora do subconjunto (mensagens intencionais) ------------

snap!(unsupported_class, "class A:\n    x = 1\n");
snap!(
    unsupported_try_except,
    "try:\n    x = 1\nexcept E:\n    pass\n"
);
snap!(unsupported_is, "x = a is b\n");
snap!(unsupported_walrus, "x = (y := 1)\n");
snap!(unsupported_ternary, "x = 1 if a else 2\n");
snap!(unsupported_matmul, "x = a @ b\n");
snap!(unsupported_string_literal, "x = 'a'\n");
snap!(unsupported_list_literal, "x = [1]\n");
snap!(unsupported_lambda, "x = lambda: 1\n");
snap!(unsupported_star_arg, "f(*a)\n");
snap!(unsupported_kwarg, "f(a=1)\n");
snap!(unsupported_leading_zero, "x = 007\n");
snap!(
    unsupported_while_else,
    "while a:\n    pass\nelse:\n    pass\n"
);
snap!(
    unsupported_for_else,
    "for x in y:\n    pass\nelse:\n    pass\n"
);
