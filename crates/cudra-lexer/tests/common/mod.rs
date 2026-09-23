// Cada ficheiro em `tests/` compila este módulo à parte, e nenhum dos dois
// usa as duas funções (dump/lex_src): sem isto, o clippy vê "dead_code" no
// binário que não usa uma delas.
#![allow(dead_code)]

use cudra_diagnostics::Diagnostic;
use cudra_lexer::{lex, Token, TokenKind};
use cudra_span::{FileId, SourceMap};

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

// Reproduz o formato do binário `dump_tokens`, usado como oráculo do
// CPython, para os testes de snapshot poderem usar o mesmo formato.
pub fn dump(src: &str) -> String {
    let mut sources = SourceMap::new();
    let file = sources.add("t.cdr".to_string(), src.to_string());
    let (tokens, diags) = lex(file, src);

    let mut out = String::new();
    for tok in &tokens {
        let text = match tok.kind {
            TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof => String::new(),
            _ => escape(&src[tok.span.lo as usize..tok.span.hi as usize]),
        };
        let (line, col) = sources.line_col(file, tok.span.lo);
        out.push_str(&format!("{:?}\t{text}\t{line}:{col}\n", tok.kind));
    }
    for diag in &diags {
        let (line, col) = diag
            .labels
            .first()
            .map(|l| sources.line_col(file, l.span.lo))
            .unwrap_or((1, 1));
        out.push_str(&format!("ERROR\t{}\t{line}:{col}\n", diag.message));
    }
    out
}

pub fn lex_src(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex(FileId(0), src)
}
