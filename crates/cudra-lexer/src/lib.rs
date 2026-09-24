mod indent;
mod token;

use cudra_diagnostics::Diagnostic;
use cudra_span::{FileId, Span};
use indent::IndentStack;
use logos::Logos;

use token::{classify_ident, Base};
pub use token::{Token, TokenKind};

pub fn lex(file: FileId, src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer::new(file, src);
    lx.skip_bom();
    lx.run();
    (lx.tokens, lx.diags)
}

struct Lexer<'a> {
    file: FileId,
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    len: usize,
    open_brackets: Vec<(u8, usize)>,
    indent_stack: IndentStack,
    at_line_start: bool,
    line_has_content: bool,
    is_first_content_line: bool,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(file: FileId, src: &'a str) -> Self {
        Lexer {
            file,
            src,
            bytes: src.as_bytes(),
            pos: 0,
            len: src.len(),
            open_brackets: Vec::new(),
            indent_stack: IndentStack::new(),
            at_line_start: true,
            line_has_content: false,
            is_first_content_line: true,
            tokens: Vec::new(),
            diags: Vec::new(),
        }
    }

    fn skip_bom(&mut self) {
        if self.bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            self.pos = 3;
        }
    }

    fn run(&mut self) {
        loop {
            self.advance_to_next_token_start();
            if self.pos >= self.len {
                self.finish_at_eof();
                return;
            }
            self.lex_one_real_token();
        }
    }

    // --- utilidades de baixo nível -----------------------------------

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn span_from(&self, lo: usize) -> Span {
        Span::new(self.file, lo as u32, self.pos as u32)
    }

    fn push_error(&mut self, span: Span, message: impl Into<String>) {
        let message = message.into();
        self.diags
            .push(Diagnostic::error(message.clone()).with_primary(span, message));
    }

    fn push_token(&mut self, kind: TokenKind, lo: usize) {
        self.tokens.push(Token {
            kind,
            span: self.span_from(lo),
        });
    }

    // --- trivia: espaços, comentários, quebras de linha, indentação ---

    fn skip_inline_whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.pos),
            Some(b' ') | Some(b'\t') | Some(b'\x0c')
        ) {
            self.pos += 1;
        }
    }

    fn skip_to_line_end(&mut self) {
        while !matches!(self.bytes.get(self.pos), None | Some(b'\n') | Some(b'\r')) {
            self.pos += 1;
        }
    }

    fn consume_newline_bytes(&mut self) {
        match self.bytes.get(self.pos) {
            Some(b'\r') => {
                self.pos += 1;
                if self.bytes.get(self.pos) == Some(&b'\n') {
                    self.pos += 1;
                }
            }
            Some(b'\n') => self.pos += 1,
            _ => {}
        }
    }

    // '\' seguido de newline (continuação normal) ou já no fim do ficheiro
    // (caso que `consume_backslash_newline` transforma num erro).
    fn at_backslash_continuation(&self) -> bool {
        matches!(
            self.bytes.get(self.pos + 1),
            Some(b'\n') | Some(b'\r') | None
        )
    }

    fn consume_backslash_newline(&mut self) {
        let bs_pos = self.pos;
        self.pos += 1; // '\'
        self.consume_newline_bytes();
        if self.pos >= self.len {
            self.push_error(
                Span::new(self.file, bs_pos as u32, bs_pos as u32 + 1),
                "continuação com '\\' no fim do ficheiro",
            );
        }
    }

    // Consome os espaços/tabs/form-feeds do início de uma linha lógica.
    // Devolve (colunas, houve_tab, posição_do_primeiro_tab).
    fn scan_indent_prefix(&mut self) -> (u32, bool, Option<usize>) {
        let mut cols = 0u32;
        let mut had_tab = false;
        let mut tab_pos = None;
        loop {
            match self.bytes.get(self.pos) {
                Some(b' ') => {
                    cols += 1;
                    self.pos += 1;
                }
                Some(b'\t') => {
                    if !had_tab {
                        had_tab = true;
                        tab_pos = Some(self.pos);
                    }
                    cols += 1;
                    self.pos += 1;
                }
                Some(b'\x0c') => {
                    // form feed: reinicia a contagem, como no Python.
                    cols = 0;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        (cols, had_tab, tab_pos)
    }

    fn apply_indent(&mut self, bs_cols: u32, cont_cols: u32, ws_lo: usize) {
        // Se a linha do `\` tem indentação própria (`bs_cols > 0`), é ela
        // que decide, e a indentação da linha de continuação (`cont_cols`)
        // é ignorada. Só quando o `\` está mesmo na coluna 0 é que a
        // decisão passa para `cont_cols`.
        let level = if bs_cols > 0 { bs_cols } else { cont_cols };
        if self.is_first_content_line {
            self.is_first_content_line = false;
            if level > 0 {
                self.push_error(
                    Span::new(self.file, ws_lo as u32, self.pos as u32),
                    "indentação inesperada",
                );
            }
            return;
        }
        let top = self.indent_stack.top();
        if level > top {
            self.indent_stack.push(level);
            self.push_token(TokenKind::Indent, self.pos);
        } else if level < top {
            let (count, inconsistent) = self.indent_stack.dedent_to(level);
            for _ in 0..count {
                self.push_token(TokenKind::Dedent, self.pos);
            }
            if inconsistent {
                self.push_error(self.span_from(self.pos), "indentação inconsistente");
            }
        }
    }

    // Processa o início de uma linha lógica (indentação + o que vem a
    // seguir). Devolve `true` quando aterra em conteúdo real (o chamador
    // deve parar e deixar `lex_one_real_token` tratar o token).
    //
    // Só se aplica indentação (e só se marca a linha como tendo conteúdo)
    // quando se encontra mesmo um token real — uma continuação '\' que só
    // leva a linhas em branco/comentário não é uma linha lógica.
    fn handle_line_start(&mut self) -> bool {
        let ws_lo = self.pos;
        let (mut cont_cols, had_tab, tab_pos) = self.scan_indent_prefix();
        let bs_cols = cont_cols; // indentação só da primeira linha física
        if had_tab {
            if let Some(tp) = tab_pos {
                self.push_error(
                    Span::new(self.file, tp as u32, tp as u32 + 1),
                    "tabs na indentação não são suportados, usa espaços",
                );
            }
        }
        loop {
            match self.peek() {
                None => return false,
                Some('#') => {
                    self.skip_to_line_end();
                    self.consume_newline_bytes();
                    return false;
                }
                Some('\n') | Some('\r') => {
                    self.consume_newline_bytes();
                    return false;
                }
                Some('\\') if self.at_backslash_continuation() => {
                    self.consume_backslash_newline();
                    let (c, had_tab2, tab_pos2) = self.scan_indent_prefix();
                    cont_cols = c;
                    if had_tab2 {
                        if let Some(tp) = tab_pos2 {
                            self.push_error(
                                Span::new(self.file, tp as u32, tp as u32 + 1),
                                "tabs na indentação não são suportados, usa espaços",
                            );
                        }
                    }
                }
                Some(_) => {
                    self.apply_indent(bs_cols, cont_cols, ws_lo);
                    self.line_has_content = true;
                    self.at_line_start = false;
                    return true;
                }
            }
        }
    }

    // Avança até ao início de um token real, tratando pelo caminho
    // comentários, linhas em branco, indentação e continuação de linha.
    // No fim, `self.pos` está ou no primeiro byte de um token real, ou no EOF.
    fn advance_to_next_token_start(&mut self) {
        loop {
            if self.pos >= self.len {
                return;
            }
            if self.at_line_start && self.open_brackets.is_empty() {
                if self.handle_line_start() {
                    return;
                }
                continue;
            }
            self.skip_inline_whitespace();
            match self.peek() {
                None => return,
                Some('#') => {
                    self.skip_to_line_end();
                    continue;
                }
                Some('\\') if self.at_backslash_continuation() => {
                    self.consume_backslash_newline();
                    continue;
                }
                Some('\n') | Some('\r') => {
                    if !self.open_brackets.is_empty() {
                        self.consume_newline_bytes();
                        continue;
                    }
                    if self.line_has_content {
                        let lo = self.pos;
                        self.consume_newline_bytes();
                        self.push_token(TokenKind::Newline, lo);
                        self.line_has_content = false;
                        self.at_line_start = true;
                        continue;
                    }
                    self.consume_newline_bytes();
                    self.at_line_start = true;
                    continue;
                }
                Some(_) => return,
            }
        }
    }

    fn finish_at_eof(&mut self) {
        if self.line_has_content {
            self.push_token(TokenKind::Newline, self.pos);
            self.line_has_content = false;
        }
        // o mais exterior é o primeiro que foi aberto (fundo da pilha).
        if let Some(&(ch, lo)) = self.open_brackets.first() {
            self.push_error(
                Span::new(self.file, lo as u32, lo as u32 + 1),
                format!("'{}' nunca foi fechado", ch as char),
            );
        }
        let count = self.indent_stack.remaining_dedents();
        for _ in 0..count {
            self.push_token(TokenKind::Dedent, self.pos);
        }
        self.push_token(TokenKind::Eof, self.pos);
    }

    // --- despacho para um token real -----------------------------------

    fn lex_one_real_token(&mut self) {
        let b = self.bytes[self.pos];
        match b {
            b'0'..=b'9' => self.lex_number(),
            b'.' if matches!(self.bytes.get(self.pos + 1), Some(d) if d.is_ascii_digit()) => {
                self.lex_number()
            }
            b'\'' | b'"' => self.lex_string(),
            b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b'u' | b'U'
                if self.string_prefix_len().is_some() =>
            {
                self.lex_string()
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => self.lex_ident(),
            b if b.is_ascii() => self.lex_operator(),
            _ => match self.peek() {
                Some(c) if c.is_alphabetic() => self.lex_nonascii_ident_error(),
                Some(_) => self.push_unexpected_char(),
                None => {}
            },
        }
    }

    fn lex_ident(&mut self) {
        let start = self.pos;
        while matches!(self.bytes.get(self.pos), Some(b) if b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.pos += 1;
        }
        // Um sufixo não-ASCII ("café") pertence ao mesmo identificador: é um
        // único erro, não um Name seguido de um erro à parte.
        if let Some(c) = self.peek() {
            if !c.is_ascii() && (c == '_' || c.is_alphanumeric()) {
                self.finish_nonascii_ident_error(start);
                return;
            }
        }
        let kind = classify_ident(&self.src[start..self.pos]);
        self.push_token(kind, start);
    }

    fn lex_nonascii_ident_error(&mut self) {
        let start = self.pos;
        self.finish_nonascii_ident_error(start);
    }

    fn finish_nonascii_ident_error(&mut self, start: usize) {
        while let Some(c) = self.peek() {
            if c == '_' || c.is_alphanumeric() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        self.push_error(
            self.span_from(start),
            "identificadores não-ASCII ainda não são suportados",
        );
    }

    fn lex_operator(&mut self) {
        let start = self.pos;
        let mut sub = Base::lexer(&self.src[start..]);
        match sub.next() {
            Some(Ok(base)) if base != Base::Ident => {
                let end = start + sub.span().end;
                self.pos = end;
                let kind = base.to_token_kind();
                match kind {
                    TokenKind::LParen | TokenKind::LBracket => {
                        let ch = if kind == TokenKind::LParen {
                            b'('
                        } else {
                            b'['
                        };
                        self.open_brackets.push((ch, start));
                    }
                    TokenKind::RParen | TokenKind::RBracket => {
                        if self.open_brackets.is_empty() {
                            let ch = if kind == TokenKind::RParen { ')' } else { ']' };
                            self.push_error(
                                self.span_from(start),
                                format!("'{ch}' sem correspondência"),
                            );
                        } else {
                            self.open_brackets.pop();
                        }
                    }
                    _ => {}
                }
                self.push_token(kind, start);
            }
            _ => self.push_unexpected_char(),
        }
    }

    fn push_unexpected_char(&mut self) {
        let start = self.pos;
        let Some(c) = self.peek() else { return };
        self.pos += c.len_utf8();
        let cp = c as u32;
        let is_control = (cp < 0x20 && !matches!(c, '\t' | '\n' | '\r')) || cp == 0x7f;
        let message = if is_control {
            format!("carácter inesperado '\\x{cp:02x}'")
        } else {
            format!("carácter inesperado '{c}'")
        };
        self.push_error(self.span_from(start), message);
    }

    // --- strings ---------------------------------------------------------

    // Se, a partir de `self.pos`, houver um prefixo válido (0-2 letras de
    // r/R/b/B/f/F/u/U) seguido logo de uma aspa, devolve o nº de bytes do
    // prefixo.
    fn string_prefix_len(&self) -> Option<usize> {
        let is_prefix_char =
            |b: u8| matches!(b, b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b'u' | b'U');
        let mut n = 0usize;
        while n < 2
            && self
                .bytes
                .get(self.pos + n)
                .copied()
                .is_some_and(is_prefix_char)
        {
            n += 1;
        }
        match self.bytes.get(self.pos + n) {
            Some(b'\'') | Some(b'"') => Some(n),
            _ => None,
        }
    }

    fn lex_string(&mut self) {
        let start = self.pos;
        let prefix_len = self.string_prefix_len().unwrap_or(0);
        self.pos += prefix_len;
        let quote = self.bytes[self.pos];
        let triple = self.bytes.get(self.pos + 1) == Some(&quote)
            && self.bytes.get(self.pos + 2) == Some(&quote);
        self.pos += if triple { 3 } else { 1 };

        loop {
            match self.bytes.get(self.pos) {
                None => {
                    self.push_error(self.span_from(start), "string sem fecho");
                    self.push_token(TokenKind::Str, start);
                    return;
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.bytes.get(self.pos) {
                        Some(b'\r') => {
                            self.pos += 1;
                            if self.bytes.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        Some(_) => {
                            if let Some(c) = self.peek() {
                                self.pos += c.len_utf8();
                            }
                        }
                        None => {}
                    }
                }
                Some(&b) if b == quote => {
                    if !triple {
                        self.pos += 1;
                        self.push_token(TokenKind::Str, start);
                        return;
                    }
                    if self.bytes.get(self.pos + 1) == Some(&quote)
                        && self.bytes.get(self.pos + 2) == Some(&quote)
                    {
                        self.pos += 3;
                        self.push_token(TokenKind::Str, start);
                        return;
                    }
                    self.pos += 1;
                }
                Some(b'\n') | Some(b'\r') if !triple => {
                    self.push_error(self.span_from(start), "string sem fecho");
                    self.push_token(TokenKind::Str, start);
                    return;
                }
                Some(_) => {
                    if let Some(c) = self.peek() {
                        self.pos += c.len_utf8();
                    } else {
                        self.pos += 1;
                    }
                }
            }
        }
    }

    // --- números -----------------------------------------------------

    // Consome um "digit run" (dígitos válidos segundo `is_digit`, com `_`
    // como separador). Devolve (viu_algum_dígito, sem_sublinhado_a_mais).
    fn scan_digit_run(&mut self, is_digit: impl Fn(u8) -> bool) -> (bool, bool) {
        let mut prev_underscore = false;
        let mut saw_digit = false;
        let mut ok = true;
        loop {
            match self.bytes.get(self.pos) {
                Some(&b) if is_digit(b) => {
                    self.pos += 1;
                    saw_digit = true;
                    prev_underscore = false;
                }
                Some(&b'_') => {
                    if prev_underscore {
                        ok = false;
                    }
                    self.pos += 1;
                    prev_underscore = true;
                }
                _ => break,
            }
        }
        if prev_underscore {
            ok = false;
        }
        (saw_digit, ok)
    }

    fn lex_number(&mut self) {
        let start = self.pos;
        if self.bytes[self.pos] == b'0' {
            if let Some(&p) = self.bytes.get(self.pos + 1) {
                let radix = match p {
                    b'x' | b'X' => Some(b'x'),
                    b'o' | b'O' => Some(b'o'),
                    b'b' | b'B' => Some(b'b'),
                    _ => None,
                };
                if let Some(radix) = radix {
                    self.lex_radix_number(start, radix);
                    return;
                }
            }
        }
        self.lex_decimal_number(start);
    }

    fn lex_radix_number(&mut self, start: usize, radix: u8) {
        self.pos += 2; // "0x"/"0o"/"0b"
        let is_digit: fn(u8) -> bool = match radix {
            b'x' => |b| b.is_ascii_hexdigit(),
            b'o' => |b| (b'0'..=b'7').contains(&b),
            _ => |b| b == b'0' || b == b'1',
        };
        let (saw_digit, ok) = self.scan_digit_run(is_digit);
        if !saw_digit {
            self.push_error(self.span_from(start), "número inválido: dígitos em falta");
            return;
        }
        if !ok {
            self.push_error(
                self.span_from(start),
                "sublinhado inválido em literal numérico",
            );
            return;
        }
        if let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_digit() {
                // dígito decimal que não é válido nesta base (ex.: '6' em 0b10106).
                let bad_pos = self.pos;
                let ch = b as char;
                self.scan_digit_run(|b| b.is_ascii_digit());
                let radix_name = if radix == b'o' { "octal" } else { "binário" };
                self.push_error(
                    Span::new(self.file, bad_pos as u32, bad_pos as u32 + 1),
                    format!("dígito inválido '{ch}' em literal {radix_name}"),
                );
                return;
            }
        }
        self.finish_number(start, TokenKind::Int, false);
    }

    fn lex_decimal_number(&mut self, start: usize) {
        let mut is_float = false;

        let intpart_start = self.pos;
        if self.bytes.get(self.pos) != Some(&b'.') {
            let (_, ok) = self.scan_digit_run(|b| b.is_ascii_digit());
            if !ok {
                self.push_error(
                    self.span_from(start),
                    "sublinhado inválido em literal numérico",
                );
                return;
            }
        }
        let intpart_end = self.pos;

        if self.bytes.get(self.pos) == Some(&b'.') {
            is_float = true;
            self.pos += 1;
            if self.bytes.get(self.pos) == Some(&b'_') {
                self.scan_digit_run(|b| b.is_ascii_digit());
                self.push_error(
                    self.span_from(start),
                    "sublinhado inválido em literal numérico",
                );
                return;
            }
            let (_, ok) = self.scan_digit_run(|b| b.is_ascii_digit());
            if !ok {
                self.push_error(
                    self.span_from(start),
                    "sublinhado inválido em literal numérico",
                );
                return;
            }
        }

        if matches!(self.bytes.get(self.pos), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if self.bytes.get(self.pos) == Some(&b'_') {
                self.scan_digit_run(|b| b.is_ascii_digit());
                self.push_error(
                    self.span_from(start),
                    "sublinhado inválido em literal numérico",
                );
                return;
            }
            let (saw, ok) = self.scan_digit_run(|b| b.is_ascii_digit());
            if !saw {
                self.push_error(self.span_from(start), "número inválido: dígitos em falta");
                return;
            }
            if !ok {
                self.push_error(
                    self.span_from(start),
                    "sublinhado inválido em literal numérico",
                );
                return;
            }
        }

        if !is_float {
            let intpart = &self.src[intpart_start..intpart_end];
            let mut digits = intpart.bytes().filter(u8::is_ascii_digit);
            if let Some(first) = digits.next() {
                if first == b'0' && digits.any(|b| b != b'0') {
                    self.push_error(
                        self.span_from(start),
                        "zeros à esquerda em literais decimais não são permitidos; usa 0o para octal",
                    );
                    return;
                }
            }
        }

        let kind = if is_float {
            TokenKind::Float
        } else {
            TokenKind::Int
        };
        self.finish_number(start, kind, true);
    }

    fn finish_number(&mut self, start: usize, kind: TokenKind, allow_complex: bool) {
        if allow_complex && matches!(self.bytes.get(self.pos), Some(b'j') | Some(b'J')) {
            self.pos += 1;
            self.push_error(
                self.span_from(start),
                "números complexos não são suportados",
            );
            return;
        }
        if let Some(c) = self.peek() {
            if c == '_' || c.is_alphabetic() {
                let mut end = self.pos;
                for c in self.src[self.pos..].chars() {
                    if c == '_' || c.is_alphanumeric() {
                        end += c.len_utf8();
                    } else {
                        break;
                    }
                }
                let glued = &self.src[self.pos..end];
                let message = format!("falta um espaço entre o número e '{glued}'");
                self.pos = end;
                self.push_error(self.span_from(start), message);
                return;
            }
        }
        self.push_token(kind, start);
    }
}
