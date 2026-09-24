#!/usr/bin/env python3
# Gera os ficheiros de referencia (oraculo CPython) para o runtime cudra.
# Reprodutivel: usa sempre random.Random(20260924).
import json
import math
import random
import re
import struct
import subprocess
import unicodedata
from pathlib import Path

SEED = 20260924
HERE = Path(__file__).resolve().parent
SRC = HERE.parent / 'src'
DIGIT_LIMIT_RE = re.compile(r"value has (\d+) digits")


def bits_hex(x: float) -> str:
    n = struct.unpack('>Q', struct.pack('>d', x))[0]
    return format(n, '016x')


def gen_float_repr_cases():
    seen = set()
    cases = []

    def add(x):
        h = bits_hex(x)
        if h in seen:
            return
        seen.add(h)
        cases.append((h, repr(x)))

    rnd = random.Random(SEED)

    for _ in range(2500):
        n = rnd.getrandbits(64)
        x = struct.unpack('>d', n.to_bytes(8, 'big'))[0]
        add(x)

    scales = [1e-300, 1e-200, 1e-100, 1e-50, 1e-10, 1e-5, 1.0, 1e5, 1e10, 1e50, 1e100, 1e200, 1e300]
    for _ in range(1300):
        scale = rnd.choice(scales)
        add(rnd.uniform(-scale, scale))

    for _ in range(600):
        n = rnd.randrange(0, 2 ** 60)
        if rnd.random() < 0.5:
            n = -n
        add(float(n))

    e = -330
    while e <= 310:
        add(float(f'1e{e}'))
        e += 1

    for base in (1e16, 1e-4, 1e15, 0.0001, 2.0 ** 53, 2.0 ** 63):
        add(math.nextafter(base, math.inf))
        add(math.nextafter(base, -math.inf))

    mandatory = [
        0.1, 1.0, -0.0, 1e16, 1e15, 123456789012345678.0, 0.0001, 0.00001,
        1 / 3, 2.5e-5, float('inf'), float('-inf'), float('nan'),
        5e-324, 1.7976931348623157e308, 9999999999999998.0, 1e22, 1e-7,
        123.456, 2.0 ** 53, 2.0 ** 63, 0.5, 100.0, 1e100,
        float('inf') - float('inf'), -float('nan'), 0.0, -1.0, 10.0, 120.0,
    ]
    for v in mandatory:
        add(v)

    return cases


def gen_unicode_tables():
    zero_cps = []
    for cp in range(0, 0x110000):
        if unicodedata.decimal(chr(cp), None) == 0:
            zero_cps.append(cp)

    ranges = []
    start = None
    for cp in range(0, 0x110000):
        if not chr(cp).isprintable():
            if start is None:
                start = cp
        elif start is not None:
            ranges.append((start, cp - 1))
            start = None
    if start is not None:
        ranges.append((start, 0x10FFFF))

    return zero_cps, ranges


def write_unicode_tables(zero_cps, ranges):
    lines = ["// gerado por gen_refs.py, nao editar", ""]

    lines.append("pub const ND_ZERO_CODEPOINTS: &[u32] = &[")
    for i in range(0, len(zero_cps), 8):
        chunk = zero_cps[i:i + 8]
        lines.append("    " + ", ".join(f"0x{c:x}" for c in chunk) + ",")
    lines.append("];")
    lines.append("")

    lines.append("pub const NON_PRINTABLE_RANGES: &[(u32, u32)] = &[")
    for lo, hi in ranges:
        lines.append(f"    (0x{lo:x}, 0x{hi:x}),")
    lines.append("];")

    out_path = SRC / 'unicode_tables.rs'
    with open(out_path, 'w', encoding='ascii', newline='\n') as f:
        f.write("\n".join(lines) + "\n")
    subprocess.run(['rustfmt', '--edition', '2021', str(out_path)], check=True)
    print(f"unicode_tables.rs: {len(zero_cps)} pontos Nd, {len(ranges)} intervalos nao-imprimiveis")


def is_diverging_char(s: str) -> bool:
    return any(
        ord(c) > 127 and not c.isspace() and unicodedata.decimal(c, None) is None
        for c in s
    )


def classify_int(s: str):
    try:
        v = int(s)
    except ValueError as e:
        msg = str(e)
        if msg.startswith("Exceeds the limit"):
            m = DIGIT_LIMIT_RE.search(msg)
            return {"too_many_digits": int(m.group(1))}
        return "ValueError"
    if is_diverging_char(s):
        return "DIVERGE"
    if -(2 ** 63) <= v <= 2 ** 63 - 1:
        return v
    return "OverflowError"


def classify_float(s: str):
    try:
        v = float(s)
    except ValueError:
        return "ValueError"
    if is_diverging_char(s):
        return "DIVERGE"
    return repr(v)


def gen_arg_parse_strings():
    strings = [
        '', ' ', '+', '-', '+-1', '1 2', '0x10', '_1', '1_', '1__0', '007', '00', '0_0',
        '１２', '  -42 ',
        '9223372036854775807', '9223372036854775808',
        '-9223372036854775808', '-9223372036854775809',
        '9223372036854775806', '-9223372036854775807',
        '1_0.5', '1._5', '1_.5', '1e1_0', '1_e10', '.5', '5.', '+.5e-2', '.', '1e',
        '0x1p3', '1,5', 'nan(123)', 'inf', 'INFINITY', 'Infinity', '-nan', '+nan',
        'infi', '1e400', '1e-400', '-0.0', '１٢', 'nan', 'NAN', '  inf  ',
        '\x1c5', '\xa05', ' 5', '5\x1c', '5\xa0', '5 ', '\x1c5\x1c',
    ]

    # R2-1: digitos Unicode Nd (arabe, devanagari, largura total, planos
    # astrais) e misturas, antes/depois da validacao gramatical.
    strings += [
        '١٢', '٣.٥', '1.5٠', '०१२', '१२३', '０１２', '２３４',
        '𝟎𝟏', '𐒠𐒡',
        '१٢０', '-١٢', '+٣.٥e١٠', '١_٢', '١__٢', '_١٢', '١٢_',
        '  १२  ', '٠٠١', '²', '²5', '5²', '١٢²',
    ]

    # R2-2: limite de 4300 digitos, na fronteira, com/sem '_', com/sem lixo,
    # com/sem espaco em volta.
    strings += [
        '1' * 4300, '1' * 4301, '0' * 4300 + '1', '0' * 5000 + '1',
        '1' * 4301 + 'x', 'x' + '1' * 4301,
        '_'.join(['1'] * 4301), '  ' + '1' * 4301 + '  ',
        '-' + '1' * 4301, '+' + '1' * 4301,
        '1' * 4300 + '_1',
    ]

    # R2-3/R2-5: caracteres de exemplo para python_repr (imprimiveis vs nao
    # imprimiveis) e string longa para truncagem a 200 codepoints.
    strings += [
        'é', 'à', '🙂', '²', ' ', ' ', '​', '﻿',
        '­', '',
        'é5', 'à5', '🙂5', ' 5', ' 5', '​5', '﻿5',
        '­5', '5',
        'x' * 300, 'x' * 199, 'x' * 200, 'x' * 201, 'x' * 198,
    ]


    alphabet = "0123456789_+-.eEinfatyINFATY \t\n\x0b\x0c\r\x1c\xa0 x"
    rnd = random.Random(SEED)
    for _ in range(2000):
        length = rnd.randint(0, 8)
        s = ''.join(rnd.choice(alphabet) for _ in range(length))
        strings.append(s)

    # dedup preservando ordem
    seen = set()
    out = []
    for s in strings:
        if s not in seen:
            seen.add(s)
            out.append(s)
    return out


def gen_arg_parse_cases():
    lines = []
    for s in gen_arg_parse_strings():
        lines.append(json.dumps({
            "s": s,
            "int": classify_int(s),
            "float": classify_float(s),
            "repr": repr(s),
        }, ensure_ascii=True))
    return lines


I64_MAX = 2**63 - 1
I64_MIN = -(2**63)


def clamp_i64(v):
    return max(I64_MIN, min(I64_MAX, v))


def f64_from_hex(h):
    return struct.unpack('>d', bytes.fromhex(h))[0]


def encode_float_result(x):
    if isinstance(x, float) and math.isnan(x):
        return "nan"
    return bits_hex(x)


def build_interesting_int_pool():
    vals = [
        0, 1, -1, 2, -2, 3, -3,
        (1 << 53), (1 << 53) + 1, (1 << 53) - 1,
        -(1 << 53), -((1 << 53) + 1), -((1 << 53) - 1),
        I64_MAX, I64_MIN, I64_MAX - 1, I64_MIN + 1,
        1285, -1285, 100, -100,
    ]
    for k in range(0, 63):
        vals.append(clamp_i64(1 << k))
        vals.append(clamp_i64(-(1 << k)))
        vals.append(clamp_i64((1 << k) + 1))
        vals.append(clamp_i64((1 << k) - 1))
    return vals


def build_interesting_float_pool():
    import sys
    vals = [
        0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 0.5, -0.5,
        float('inf'), float('-inf'), float('nan'),
        5e-324, -5e-324, sys.float_info.max, -sys.float_info.max,
        0.1, -0.1, 1e308, -1e308, 1e-308, -1e-308,
        2.0 ** 52, 2.0 ** 53, 2.0 ** 63, -(2.0 ** 63), 2.0 ** 52 + 1, 2.0 ** 53 + 2,
        3.0, -3.0, 5.5, -5.5, 100.0, -100.0, 10.0, -10.0, 1285.0, -1285.0,
    ]
    for k in range(-20, 21):
        vals.append(2.0 ** k)
        vals.append(-(2.0 ** k))
    for k in range(0, 20):
        vals.append(2.0 ** k + 1)
        vals.append(2.0 ** k - 1)
        vals.append(-(2.0 ** k) - 1)
        vals.append(-(2.0 ** k) + 1)
    for v in [1, 2, 3, 4, 5, 7, 10, 100, 1000, 123456]:
        vals.append(float(v))
        vals.append(-float(v))
    return vals


def gen_uniform_f64_bits(rnd):
    n = rnd.getrandbits(64)
    return struct.unpack('>d', n.to_bytes(8, 'big'))[0]


def gen_uniform_i64(rnd):
    return rnd.randint(I64_MIN, I64_MAX)


def compute_int_truediv(a, b):
    if b == 0:
        return {"error": "ZeroDivisionError: division by zero"}
    return encode_float_result(a / b)


def gen_int_truediv_cases():
    cases = []
    seen = set()

    def add(a, b):
        key = (a, b)
        if key not in seen:
            seen.add(key)
            cases.append(key)

    mandatory = [
        (9223372036854775807, 1285),
        (-9223372036854775808, 3),
        (9007199254740993, 1),
        (-9223372036854775807, -1),
        (0, -5), (0, 5), (0, -1), (0, 1),
        (0, 0), (1, 0), (-1, 0), (I64_MAX, 0), (I64_MIN, 0),
    ]
    for a, b in mandatory:
        add(a, b)

    pool = build_interesting_int_pool()
    for b in pool:
        for a in pool[:40]:
            add(a, b)

    rnd = random.Random(SEED + 1)
    for _ in range(20000):
        a = gen_uniform_i64(rnd)
        b = gen_uniform_i64(rnd)
        add(a, b)

    for _ in range(5000):
        a = clamp_i64(rnd.randint((1 << 53) + 1, I64_MAX) * rnd.choice([1, -1]))
        b = clamp_i64(rnd.randint((1 << 53) + 1, I64_MAX) * rnd.choice([1, -1]))
        add(a, b)

    for _ in range(5000):
        a = clamp_i64(rnd.randint((1 << 53) - 1000, (1 << 53) + 1000) * rnd.choice([1, -1]))
        b = clamp_i64(rnd.randint((1 << 53) - 1000, (1 << 53) + 1000) * rnd.choice([1, -1]))
        add(a, b)

    return cases


def compute_float_floordiv(a, b):
    if b == 0.0:
        return {"error": "ZeroDivisionError: float floor division by zero"}
    return encode_float_result(a // b)


def compute_float_mod(a, b):
    if b == 0.0:
        return {"error": "ZeroDivisionError: float modulo"}
    return encode_float_result(a % b)


def gen_float_pair_cases(extra_mandatory, seed_offset):
    cases = []
    seen = set()

    def add(a, b):
        key = (bits_hex(a), bits_hex(b))
        if key not in seen:
            seen.add(key)
            cases.append((a, b))

    for a, b in extra_mandatory:
        add(a, b)

    pool = build_interesting_float_pool()
    for b in pool:
        for a in pool[:40]:
            add(a, b)

    rnd = random.Random(SEED + seed_offset)
    for _ in range(10000):
        add(gen_uniform_f64_bits(rnd), gen_uniform_f64_bits(rnd))
    for _ in range(10000):
        add(rnd.choice(pool), rnd.choice(pool))

    return cases


def compute_fpow(x, y):
    # x infinito e tratado por um caso especial mais cedo, tanto no
    # float_pow do CPython como na nossa implementacao — nunca chega a
    # complex, mesmo com expoente fracionario. So x finito negativo diverge.
    if x < 0 and math.isfinite(x) and not math.isnan(y) and not math.isinf(y) and y != math.floor(y):
        # O CPython delega em complex_pow (_Py_c_pow): len = pow(|x|, y). Se
        # o modulo estourar para infinito, a mensagem real e
        # "OverflowError: complex exponentiation" (comparavel); senao o
        # resultado e um complex genuino (incluindo 0j por underflow), que
        # continua por comparar (a nossa ValueError documentada). Usamos o
        # proprio operador ** (nao math.pow) porque e ele que reproduz o
        # overflow do float_pow do CPython (levanta OverflowError em vez de
        # devolver inf).
        try:
            length_is_inf = math.isinf(abs(x) ** y)
        except OverflowError:
            length_is_inf = True
        if length_is_inf:
            return {"error": "OverflowError: complex exponentiation"}
        return "DIVERGE"
    try:
        v = x ** y
    except ZeroDivisionError:
        return {"error": "ZeroDivisionError: 0.0 cannot be raised to a negative power"}
    except OverflowError:
        return {"error": "OverflowError: (34, 'Numerical result out of range')"}
    if isinstance(v, complex):
        return "DIVERGE"
    return encode_float_result(v)


def compute_cmp_int_float(i, f):
    if math.isnan(f):
        return 2
    if i < f:
        return -1
    if i > f:
        return 1
    return 0


def gen_cmp_int_float_cases():
    cases = []
    seen = set()

    def add(i, f):
        key = (i, bits_hex(f))
        if key not in seen:
            seen.add(key)
            cases.append((i, f))

    mandatory = [
        (2 ** 53 + 1, 9007199254740992.0),
        (9223372036854775807, 9.223372036854776e18),
        (I64_MIN, -9.223372036854776e18),
        (I64_MIN, -0.0),
        (I64_MIN, float('inf')),
        (I64_MIN, float('-inf')),
        (0, 0.0), (0, -0.0), (0, float('nan')),
    ]
    for i, f in mandatory:
        add(i, f)

    int_pool = build_interesting_int_pool()
    float_pool = build_interesting_float_pool()
    for i in int_pool:
        for f in float_pool[:30]:
            add(i, f)

    rnd = random.Random(SEED + 5)
    for _ in range(10000):
        add(gen_uniform_i64(rnd), gen_uniform_f64_bits(rnd))
    for _ in range(10000):
        add(rnd.choice(int_pool), rnd.choice(float_pool))

    return cases


def compute_float_to_int(f):
    if math.isnan(f):
        return {"error": "ValueError: cannot convert float NaN to integer"}
    if math.isinf(f):
        return {"error": "OverflowError: cannot convert float infinity to integer"}
    t = math.trunc(f)
    if not (I64_MIN <= t <= I64_MAX):
        return {"error": "OverflowError: float too large to convert to int"}
    return t


def gen_float_to_int_cases():
    cases = []
    seen = set()

    def add(f):
        key = bits_hex(f)
        if key not in seen:
            seen.add(key)
            cases.append(f)

    mandatory = [
        -9223372036854775808.0, 9223372036854775808.0, 9223372036854775807.0,
        -9223372036854775809.0, float('nan'), float('inf'), float('-inf'),
        0.0, -0.0, 1.5, -1.5,
    ]
    for f in mandatory:
        add(f)

    pool = build_interesting_float_pool()
    for f in pool:
        add(f)

    rnd = random.Random(SEED + 6)
    for _ in range(10000):
        add(gen_uniform_f64_bits(rnd))
    for _ in range(10000):
        add(rnd.choice(pool))

    return cases


def gen_num_lines():
    lines = []

    for a, b in gen_int_truediv_cases():
        result = compute_int_truediv(a, b)
        lines.append(json.dumps(
            {"fn": "int_truediv", "a": a, "b": b, "result": result}, ensure_ascii=True))

    floordiv_mod_mandatory = [
        (1.0, 0.1), (-4.0, 2.0), (4.0, -2.0), (-0.0, 1.0), (0.0, -1.0), (-7.0, 2.0),
        (float('inf'), 1.0), (1.0, float('inf')), (-1.0, float('inf')),
        (1e308, 1e-308), (-1e-300, 1e300),
        (1.0, 0.0), (1.0, -0.0), (-5.0, 0.0),
    ]
    for a, b in gen_float_pair_cases(floordiv_mod_mandatory, 2):
        lines.append(json.dumps({
            "fn": "float_floordiv", "a": bits_hex(a), "b": bits_hex(b),
            "result": compute_float_floordiv(a, b),
        }, ensure_ascii=True))
        lines.append(json.dumps({
            "fn": "float_mod", "a": bits_hex(a), "b": bits_hex(b),
            "result": compute_float_mod(a, b),
        }, ensure_ascii=True))

    fpow_mandatory = [
        (-1.0, float('inf')), (0.5, float('inf')), (0.5, float('-inf')), (-0.5, float('-inf')),
        (1.5, float('-inf')), (-1.5, float('inf')), (float('inf'), -1.0), (0.0, 0.0),
        (float('nan'), 0.0), (1.0, float('nan')),
        (float('-inf'), 0.5), (float('-inf'), 3.0), (float('-inf'), -3.0), (float('-inf'), -2.0),
        (-0.0, 3.0), (-0.0, 2.0), (2.0, -1080.0),
        (10.0, 400.0), (-2.0, 1025.0),
        (0.0, -1.0), (0.0, -1.5), (-0.0, -1.0),
        (-2.0, 0.5), (-8.0, 1.0 / 3.0), (-2.0, -0.5),
        # ronda 3b: overflow do modulo em complex_pow -> OverflowError real.
        (-3.0, 946683.3866556706), (-0.1, -83637.9596037151),
        (-1.7976931348623157e+308, 4.5), (-5e-324, -426738.27163369756),
        (-6.800000000000001, 4503599627370495.5), (-1.4156629567185994e+206, 39.5),
        # continuam a dar complex genuino (underflow/subnormal, sem erro real).
        (-0.5, 1000.5), (-0.5, 5000.5),
    ]
    for x, y in gen_float_pair_cases(fpow_mandatory, 3):
        lines.append(json.dumps({
            "fn": "fpow", "x": bits_hex(x), "y": bits_hex(y),
            "result": compute_fpow(x, y),
        }, ensure_ascii=True))

    for i, f in gen_cmp_int_float_cases():
        lines.append(json.dumps({
            "fn": "cmp_int_float", "i": i, "f": bits_hex(f),
            "result": compute_cmp_int_float(i, f),
        }, ensure_ascii=True))

    for f in gen_float_to_int_cases():
        lines.append(json.dumps({
            "fn": "float_to_int", "f": bits_hex(f),
            "result": compute_float_to_int(f),
        }, ensure_ascii=True))

    return lines


def main():
    zero_cps, ranges = gen_unicode_tables()
    write_unicode_tables(zero_cps, ranges)

    float_cases = gen_float_repr_cases()
    with open(HERE / 'float_repr_ref.txt', 'w', encoding='ascii', newline='\n') as f:
        for h, r in float_cases:
            f.write(f"{h}\t{r}\n")
    print(f"float_repr_ref.txt: {len(float_cases)} casos")

    arg_lines = gen_arg_parse_cases()
    with open(HERE / 'arg_parse_ref.txt', 'w', encoding='ascii', newline='\n') as f:
        for line in arg_lines:
            f.write(line + '\n')
    print(f"arg_parse_ref.txt: {len(arg_lines)} casos")

    num_lines = gen_num_lines()
    with open(HERE / 'num_ref.txt', 'w', encoding='ascii', newline='\n') as f:
        for line in num_lines:
            f.write(line + '\n')
    print(f"num_ref.txt: {len(num_lines)} casos")


if __name__ == '__main__':
    main()
