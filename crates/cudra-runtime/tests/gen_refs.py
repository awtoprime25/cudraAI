#!/usr/bin/env python3
# Gera os ficheiros de referencia (oraculo CPython) para o runtime cudra.
# Reprodutivel: usa sempre random.Random(20260924).
import json
import math
import random
import re
import struct
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


if __name__ == '__main__':
    main()
