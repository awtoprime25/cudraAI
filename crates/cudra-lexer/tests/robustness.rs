// Testa `lex` sobre todos os prefixos (cortados em cada fronteira de char)
// de vários programas, incluindo alguns propositadamente inválidos/estranhos.
// Em nenhum caso pode: entrar em pânico, produzir #Indent != #Dedent,
// produzir dois Newline seguidos, ou terminar sem Eof como último token.
mod common;
use common::lex_src;
use cudra_lexer::TokenKind;

const PROGRAMS: &[&str] = &[
    "x = 1\n",
    "if x:\n    y = 1\nelif w:\n    pass\nelse:\n    y = 2\n",
    "for i in range(3):\n    for j in range(3):\n        x = i + j\n",
    "def f(a: int, b: float) -> float:\n    return a + b\n",
    "while i < 10:\n    i += 1\n",
    "x = (1,\n  # c\n  2,\n  3)\n",
    "x = 1 + \\\n    2\n",
    "x = 1\r\ny = 2\r\n",
    "a = 0x_ff\nb = 0b_1\nc = 0o17\nd = 1_000\n",
    "a = 1.5\nb = 1.\nc = .5\nd = 1e10\ne = 1E-3\nf = 1_0.5e-3\n",
    "a = 'abc'\nb = \"abc\"\nc = 'x\\'y'\n",
    "a = '''l1\nl2'''\nb = rb'raw'\nc = f\"fs\"\n",
    "import cudra\nfrom cudra import arg_int, arg_float\n",
    "x += 1\nx -= 1\nx *= 1\nx //= 1\nx **= 1\nx <<= 1\n",
    "z = a < b <= c == d != e > f >= g\n",
    "z = a & b | c ^ d << e >> ~f\n",
    "class x:\n    pass\nlambda_ = 1\n",
    "a = True\nb = False\nc = None\n",
    "",
    "# so isto\n\n   \n# mais\n",
    "if a:\n    if b:\n        if c:\n            x = 1",
    "if x:\n\ty = 1\n    z = 2\n",
    "if x:\n   y = 1\n  z = 2\n",
    "a = 1__0\nb = 10_\nc = 0x\nd = 1e\ne = 0777\nf = 1j\ng = 1if\n",
    "a = 1 ! 2\nb = $\nc = ?\nd = `\n",
    "x = 'abc\ny = 1\n",
    "café = 1\n",
    "x = 1)\ny = [2\n",
    "    x = 1\n",
    "def f(a: int) -> int:\n    if a > 0:\n        return a\n    return -a\n",
    "x = y . z [ 0 ] ( 1 , 2 )\n",
    "x = 1\n\n\ny = 2\n\n",
    "if True:\n    pass\nif False:\n    pass\n",
];

fn check_invariants(src: &str) {
    let (tokens, _diags) = lex_src(src);

    assert_eq!(
        tokens.last().map(|t| t.kind),
        Some(TokenKind::Eof),
        "último token não é Eof para {src:?}"
    );

    let indents = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Indent)
        .count();
    let dedents = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Dedent)
        .count();
    assert_eq!(
        indents, dedents,
        "Indent != Dedent para {src:?}: {indents} vs {dedents}"
    );

    for pair in tokens.windows(2) {
        assert!(
            !(pair[0].kind == TokenKind::Newline && pair[1].kind == TokenKind::Newline),
            "dois Newline seguidos para {src:?}"
        );
    }
}

#[test]
fn at_least_30_programs_covered() {
    assert!(PROGRAMS.len() >= 30, "só há {} programas", PROGRAMS.len());
}

#[test]
fn full_programs_never_panic_and_respect_invariants() {
    for src in PROGRAMS {
        check_invariants(src);
    }
}

#[test]
fn every_char_boundary_prefix_never_panics_and_respects_invariants() {
    for src in PROGRAMS {
        let mut boundaries: Vec<usize> = src.char_indices().map(|(i, _)| i).collect();
        boundaries.push(src.len());
        for &b in &boundaries {
            check_invariants(&src[..b]);
        }
    }
}
