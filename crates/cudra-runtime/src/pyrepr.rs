// repr() de string ao estilo Python, usado nas mensagens de erro de
// cudra_arg_int/cudra_arg_float (ex.: "invalid literal for int() with base
// 10: '1__0'"). Cobre ASCII imprimivel tal e qual, e escapa o resto
// (controlo, separadores unicode, etc.) como \n \r \t ou \xXX/\uXXXX/\UXXXXXXXX.
pub fn python_repr(s: &str) -> String {
    let has_single = s.contains('\'');
    let has_double = s.contains('"');
    let quote = if has_single && !has_double { '"' } else { '\'' };

    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        if c == '\\' {
            out.push_str("\\\\");
        } else if c == quote {
            out.push('\\');
            out.push(c);
        } else if c == '\n' {
            out.push_str("\\n");
        } else if c == '\r' {
            out.push_str("\\r");
        } else if c == '\t' {
            out.push_str("\\t");
        } else if (' '..='~').contains(&c) {
            out.push(c);
        } else {
            escape_unicode(c, &mut out);
        }
    }
    out.push(quote);
    out
}

fn escape_unicode(c: char, out: &mut String) {
    let cp = c as u32;
    if cp < 0x100 {
        out.push_str(&format!("\\x{cp:02x}"));
    } else if cp < 0x10000 {
        out.push_str(&format!("\\u{cp:04x}"));
    } else {
        out.push_str(&format!("\\U{cp:08x}"));
    }
}
