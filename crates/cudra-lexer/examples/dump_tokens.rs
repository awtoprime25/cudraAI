use std::env;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

use cudra_lexer::{lex, TokenKind};
use cudra_span::SourceMap;

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

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let Some(path) = args.get(1) else {
        eprintln!("uso: dump_tokens <ficheiro>");
        return ExitCode::from(1);
    };

    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            let _ = writeln!(out, "ERROR\tnão foi possível ler o ficheiro: {e}\t1:1");
            let _ = out.flush();
            return ExitCode::from(1);
        }
    };

    let src = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => {
            let pos = e.utf8_error().valid_up_to();
            let byte = e.as_bytes()[pos];
            let _ = writeln!(
                out,
                "ERROR\tficheiro não é UTF-8 válido (byte 0x{byte:02X} na posição {pos})\t1:1"
            );
            let _ = out.flush();
            return ExitCode::from(1);
        }
    };

    let mut sources = SourceMap::new();
    let file = sources.add(path.clone(), src.clone());

    let (tokens, diags) = lex(file, &src);
    let exit = if diags.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    };

    for tok in &tokens {
        let text = match tok.kind {
            TokenKind::Indent | TokenKind::Dedent | TokenKind::Eof => String::new(),
            _ => escape(&src[tok.span.lo as usize..tok.span.hi as usize]),
        };
        let (line, col) = sources.line_col(file, tok.span.lo);
        if writeln!(out, "{:?}\t{text}\t{line}:{col}", tok.kind).is_err() {
            return exit;
        }
    }

    for diag in &diags {
        let (line, col) = diag
            .labels
            .first()
            .map(|l| sources.line_col(file, l.span.lo))
            .unwrap_or((1, 1));
        if writeln!(out, "ERROR\t{}\t{line}:{col}", diag.message).is_err() {
            return exit;
        }
    }

    let _ = out.flush();
    exit
}
