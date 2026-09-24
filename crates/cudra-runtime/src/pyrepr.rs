// repr() de string ao estilo Python, usado nas mensagens de erro de
// cudra_arg_int/cudra_arg_float (ex.: "invalid literal for int() with base
// 10: '1__0'"). Um caracter e mostrado tal e qual sse for imprimivel
// (str.isprintable()); caso contrario e escapado como \n \r \t ou
// \xXX/\uXXXX/\UXXXXXXXX. Trabalha com codepoints (u32), nao com char do
// Rust, para poder representar surrogates soltos vindos do argv.
use crate::unicode::is_printable;

pub fn python_repr(cps: &[u32]) -> String {
    codepoints_to_string(&build_repr(cps))
}

/// Como python_repr, mas trunca a %.200R do CPython: os primeiros
/// `max_chars` codepoints do repr completo (incluindo a aspa de abertura),
/// sem aspa de fecho quando a truncagem realmente acontece.
pub fn python_repr_truncated(cps: &[u32], max_chars: usize) -> String {
    let full = build_repr(cps);
    if full.len() <= max_chars {
        codepoints_to_string(&full)
    } else {
        codepoints_to_string(&full[..max_chars])
    }
}

fn build_repr(cps: &[u32]) -> Vec<u32> {
    let single = '\'' as u32;
    let double = '"' as u32;
    let has_single = cps.contains(&single);
    let has_double = cps.contains(&double);
    let quote = if has_single && !has_double {
        double
    } else {
        single
    };

    let mut out = Vec::with_capacity(cps.len() + 2);
    out.push(quote);
    for &cp in cps {
        if cp == '\\' as u32 {
            out.push('\\' as u32);
            out.push('\\' as u32);
        } else if cp == quote {
            out.push('\\' as u32);
            out.push(cp);
        } else if cp == '\n' as u32 {
            out.push('\\' as u32);
            out.push('n' as u32);
        } else if cp == '\r' as u32 {
            out.push('\\' as u32);
            out.push('r' as u32);
        } else if cp == '\t' as u32 {
            out.push('\\' as u32);
            out.push('t' as u32);
        } else if is_printable(cp) {
            out.push(cp);
        } else {
            escape_unicode(cp, &mut out);
        }
    }
    out.push(quote);
    out
}

fn escape_unicode(cp: u32, out: &mut Vec<u32>) {
    let hex = if cp < 0x100 {
        format!("\\x{cp:02x}")
    } else if cp < 0x10000 {
        format!("\\u{cp:04x}")
    } else {
        format!("\\U{cp:08x}")
    };
    out.extend(hex.chars().map(|c| c as u32));
}

/// Junta codepoints num String. So e chamada sobre sequencias que a propria
/// build_repr produziu — nunca contem surrogates soltos, porque surrogates
/// nunca sao imprimiveis e por isso vao sempre pela via de escape (so ASCII).
fn codepoints_to_string(cps: &[u32]) -> String {
    let mut s = String::with_capacity(cps.len());
    for &cp in cps {
        s.push(char::from_u32(cp).expect("build_repr nunca produz surrogates soltos"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cps(s: &str) -> Vec<u32> {
        s.chars().map(|c| c as u32).collect()
    }

    #[test]
    fn printable_non_ascii_shown_raw() {
        assert_eq!(python_repr(&cps("é")), "'é'");
        assert_eq!(python_repr(&cps("à\u{0300}")), "'à\u{0300}'");
        assert_eq!(python_repr(&cps("🙂")), "'🙂'");
        assert_eq!(python_repr(&cps("²")), "'²'");
    }

    #[test]
    fn non_printable_escaped() {
        assert_eq!(python_repr(&cps("\u{2028}")), "'\\u2028'");
        assert_eq!(python_repr(&cps("\u{a0}")), "'\\xa0'");
        assert_eq!(python_repr(&cps("\u{200b}")), "'\\u200b'");
        assert_eq!(python_repr(&cps("\u{feff}")), "'\\ufeff'");
        assert_eq!(python_repr(&cps("\u{ad}")), "'\\xad'");
        assert_eq!(python_repr(&cps("\u{7f}")), "'\\x7f'");
    }

    #[test]
    fn truncation_at_200_codepoints() {
        let s = "x".repeat(300);
        let full = python_repr(&cps(&s));
        assert_eq!(full.chars().count(), 302);

        let truncated = python_repr_truncated(&cps(&s), 200);
        assert_eq!(truncated.chars().count(), 200);
        assert!(!truncated.ends_with('\''));

        // string curta o suficiente para nao ser truncada continua com aspa
        // de fecho.
        let short = "x".repeat(198);
        assert_eq!(
            python_repr_truncated(&cps(&short), 200),
            python_repr(&cps(&short))
        );
        assert!(python_repr_truncated(&cps(&short), 200).ends_with('\''));
    }

    #[test]
    fn lone_surrogate_escaped() {
        assert_eq!(python_repr(&[0xDCFFu32]), "'\\udcff'");
    }
}
