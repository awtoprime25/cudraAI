// Classificacao Unicode (categoria Nd e isprintable()) a partir das tabelas
// geradas por gen_refs.py, mais a descodificacao de argv com o algoritmo
// surrogateescape do Python. Trabalha sempre com "codepoints" (u32), nao com
// char do Rust, porque surrogates soltos (0xD800-0xDFFF) sao strings Python
// validas mas nao sao valores escalares Unicode validos em Rust.
use crate::unicode_tables::{ND_ZERO_CODEPOINTS, NON_PRINTABLE_RANGES};

/// Valor decimal (0-9) de `cp` se pertencer a categoria Nd, ao estilo de
/// unicodedata.decimal(). Cada digito Nd faz parte de uma corrida contigua
/// de exatamente 10 codepoints (0 a 9), por isso basta procurar o inicio da
/// corrida a que `cp` pertence.
pub fn digit_value(cp: u32) -> Option<u8> {
    let idx = match ND_ZERO_CODEPOINTS.binary_search(&cp) {
        Ok(i) => i,
        Err(0) => return None,
        Err(i) => i - 1,
    };
    let zero = ND_ZERO_CODEPOINTS[idx];
    if cp >= zero && cp - zero <= 9 {
        Some((cp - zero) as u8)
    } else {
        None
    }
}

/// str.isprintable() para um unico codepoint.
pub fn is_printable(cp: u32) -> bool {
    match NON_PRINTABLE_RANGES.binary_search_by(|&(lo, _)| lo.cmp(&cp)) {
        Ok(_) => false,
        Err(0) => true,
        Err(i) => {
            let (_, hi) = NON_PRINTABLE_RANGES[i - 1];
            cp > hi
        }
    }
}

/// Descodifica bytes UTF-8 possivelmente invalidos com o algoritmo
/// surrogateescape do Python: cada byte individualmente invalido vira o
/// codepoint 0xDC00 + byte (um surrogate baixo, nao um char valido em Rust).
pub fn utf8_surrogateescape_decode(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.extend(s.chars().map(|c| c as u32));
                break;
            }
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                let valid = std::str::from_utf8(&rest[..valid_up_to]).expect("prefixo valido");
                out.extend(valid.chars().map(|c| c as u32));
                // um byte invalido de cada vez, mesmo que error_len() diga mais.
                out.push(0xDC00 + rest[valid_up_to] as u32);
                rest = &rest[valid_up_to + 1..];
            }
        }
    }
    out
}

/// Converte uma &str Rust (sempre UTF-8 valida) para codepoints.
pub fn str_to_codepoints(s: &str) -> Vec<u32> {
    s.chars().map(|c| c as u32).collect()
}
