mod common;
use common::parse_src;

// Truncar um programa em qualquer posição nunca pode fazer o parser
// rebentar (panic) — só produzir erros, o que for razoável.
const SAMPLES: &[&str] = &[
    "def f(a: int, b: float) -> float:\n    return a / b\n",
    "if a:\n    x = 1\nelif b:\n    x = 2\nelse:\n    x = 3\n",
    "for i in range(10):\n    if i:\n        continue\n    else:\n        break\n",
    "while x < 10:\n    x += 1\n    if x == 5:\n        break\n",
    "x = a|b^c&d<<e>>f+g-h*i/j//k%l**m\n",
    "@a.b(1)(2)\n@c\ndef f(): pass\n",
    "class A:\n    x = 1\n    def f(self): pass\ny = 2\n",
    "try:\n    x = 1\nexcept E:\n    pass\nelse:\n    y = 1\nfinally:\n    z = 1\nw = 2\n",
    "x: Array[f32, i32] = f(1, 2.0)\nprint(x)\n",
    "from cudra import arg_int, arg_float\nx = arg_int()\n",
];

#[test]
fn truncation_at_every_position_never_panics() {
    for src in SAMPLES {
        for k in 0..=src.len() {
            // só cortar em fronteiras válidas de char UTF-8
            if !src.is_char_boundary(k) {
                continue;
            }
            let _ = parse_src(&src[..k]);
        }
    }
}
