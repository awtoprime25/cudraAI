mod indent;
mod token;

use cudra_diagnostics::Diagnostic;
use cudra_span::{FileId, Span};
use indent::IndentStack;
use logos::Logos;

use token::{classify_ident, Base};
pub use token::{Token, TokenKind};

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

// Resultado de uma varredura de texto literal de f-string (parte "de fora",
// ou o texto literal de um format spec).
enum FScan {
    Closed,
    FieldOpen,
    Eof,
}

pub fn lex(file: FileId, src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer::new(file, src);
    lx.skip_bom();
    lx.check_null_bytes();
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
    bracket_over_limit: bool,
    indent_stack: IndentStack,
    indent_over_limit: bool,
    at_line_start: bool,
    line_has_content: bool,
    is_first_content_line: bool,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
    diag_limit_hit: bool,
}

const MAX_DIAGS: usize = 1000;

// CPython MAXLEVEL: medido empiricamente no WSL — 200 parênteses/colchetes/
// chavetas abertos ao mesmo tempo cabem (partilhado entre os três tipos, e
// também pelos que estão dentro da parte de expressão de uma f-string), o
// 201º dá "too many nested parentheses".
const MAXLEVEL: usize = 200;

impl<'a> Lexer<'a> {
    fn new(file: FileId, src: &'a str) -> Self {
        Lexer {
            file,
            src,
            bytes: src.as_bytes(),
            pos: 0,
            len: src.len(),
            open_brackets: Vec::new(),
            bracket_over_limit: false,
            indent_stack: IndentStack::new(),
            indent_over_limit: false,
            at_line_start: true,
            line_has_content: false,
            is_first_content_line: true,
            tokens: Vec::new(),
            diags: Vec::new(),
            diag_limit_hit: false,
        }
    }

    fn skip_bom(&mut self) {
        if self.bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            self.pos = 3;
        }
    }

    fn check_null_bytes(&mut self) {
        if let Some(pos) = self.bytes.iter().position(|&b| b == 0) {
            self.push_error(
                Span::new(self.file, pos as u32, pos as u32 + 1),
                "o ficheiro contém bytes nulos (0x00)",
            );
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
        self.push_diag(Diagnostic::error(message.clone()).with_primary(span, message));
    }

    fn push_diag(&mut self, diag: Diagnostic) {
        if self.diag_limit_hit {
            return;
        }
        if self.diags.len() + 1 >= MAX_DIAGS {
            self.diags.push(Diagnostic::error(format!(
                "demasiados erros léxicos ({MAX_DIAGS}); os restantes foram omitidos"
            )));
            self.diag_limit_hit = true;
            return;
        }
        self.diags.push(diag);
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
            Some(b' ') | Some(b'\t') | Some(b'\x0c') | Some(b'\0')
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
                Some(b' ') | Some(b'\0') => {
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
            if self.indent_stack.push(level) {
                self.push_token(TokenKind::Indent, self.pos);
            } else if !self.indent_over_limit {
                self.indent_over_limit = true;
                self.push_error(
                    self.span_from(self.pos),
                    format!(
                        "demasiados níveis de indentação (máximo {})",
                        indent::MAXINDENT - 1
                    ),
                );
            }
        } else if level < top {
            let (count, inconsistent) = self.indent_stack.dedent_to(level);
            if count > 0 {
                self.indent_over_limit = false;
            }
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

    // Regista a abertura de um parêntese/colchete/chaveta (a nível de topo
    // ou dentro da parte de expressão de uma f-string) e verifica o limite
    // do L8 (MAXLEVEL), reportando uma única vez por excesso.
    fn push_bracket_depth(&mut self, ch: u8, pos: usize) {
        self.open_brackets.push((ch, pos));
        if self.open_brackets.len() > MAXLEVEL {
            if !self.bracket_over_limit {
                self.bracket_over_limit = true;
                self.push_error(
                    Span::new(self.file, pos as u32, pos as u32 + 1),
                    format!("demasiados parênteses aninhados (máximo {MAXLEVEL})"),
                );
            }
        } else {
            self.bracket_over_limit = false;
        }
    }

    fn pop_bracket_depth(&mut self) -> Option<(u8, usize)> {
        let popped = self.open_brackets.pop();
        if self.open_brackets.len() <= MAXLEVEL {
            self.bracket_over_limit = false;
        }
        popped
    }

    fn opener_for_closer(ch: u8) -> u8 {
        match ch {
            b')' => b'(',
            b']' => b'[',
            _ => b'{',
        }
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
                    TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                        let ch = match kind {
                            TokenKind::LParen => b'(',
                            TokenKind::LBracket => b'[',
                            _ => b'{',
                        };
                        self.push_bracket_depth(ch, start);
                    }
                    TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                        let close_ch = match kind {
                            TokenKind::RParen => b')',
                            TokenKind::RBracket => b']',
                            _ => b'}',
                        };
                        if self.open_brackets.is_empty() {
                            self.push_error(
                                self.span_from(start),
                                format!("'{}' sem correspondência", close_ch as char),
                            );
                        } else {
                            let (open_ch, open_pos) = self.pop_bracket_depth().unwrap();
                            let expected = Self::opener_for_closer(close_ch);
                            if open_ch != expected {
                                let msg = format!(
                                    "'{}' não corresponde a '{}'",
                                    close_ch as char, open_ch as char
                                );
                                let diag = Diagnostic::error(msg.clone())
                                    .with_primary(self.span_from(start), msg)
                                    .with_secondary(
                                        Span::new(self.file, open_pos as u32, open_pos as u32 + 1),
                                        "aberto aqui",
                                    );
                                self.push_diag(diag);
                            }
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
        while self.is_unexpected_char_start(self.pos) {
            let Some(next) = self.peek() else { break };
            self.pos += next.len_utf8();
        }
        let cp = c as u32;
        let is_control = (cp < 0x20 && !matches!(c, '\t' | '\n' | '\r')) || cp == 0x7f;
        let message = if is_control {
            format!("carácter inesperado '\\x{cp:02x}'")
        } else {
            format!("carácter inesperado '{c}'")
        };
        self.push_error(self.span_from(start), message);
    }

    fn is_unexpected_char_start(&self, pos: usize) -> bool {
        if pos >= self.len {
            return false;
        }
        match self.bytes[pos] {
            b' ' | b'\t' | b'\x0c' | b'\0' | b'\n' | b'\r' => false,
            b'0'..=b'9' => false,
            b'.' if matches!(self.bytes.get(pos + 1), Some(d) if d.is_ascii_digit()) => false,
            b'\'' | b'"' => false,
            b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b'u' | b'U'
                if self.string_prefix_len_at(pos).is_some() =>
            {
                false
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => false,
            b if b.is_ascii() => {
                let mut sub = Base::lexer(&self.src[pos..]);
                !matches!(sub.next(), Some(Ok(base)) if base != Base::Ident)
            }
            _ => match self.src[pos..].chars().next() {
                Some(c) => !c.is_alphabetic(),
                None => false,
            },
        }
    }

    // --- strings ---------------------------------------------------------

    // Se, a partir de `self.pos`, houver um prefixo válido de string ("",
    // r, u, b, f, rb, br, fr, rf — em qualquer combinação de maiúsculas e
    // minúsculas; sem t-strings) seguido logo de uma aspa, devolve o nº de
    // bytes do prefixo. Qualquer outra combinação de letras não conta como
    // prefixo (fica para `lex_ident` e a aspa começa uma string à parte).
    fn string_prefix_len(&self) -> Option<usize> {
        self.string_prefix_len_at(self.pos)
    }

    fn string_prefix_len_at(&self, pos: usize) -> Option<usize> {
        let is_prefix_char =
            |b: u8| matches!(b, b'r' | b'R' | b'b' | b'B' | b'f' | b'F' | b'u' | b'U');
        let mut n = 0usize;
        while n < 2 && self.bytes.get(pos + n).copied().is_some_and(is_prefix_char) {
            n += 1;
        }
        if !matches!(self.bytes.get(pos + n), Some(b'\'') | Some(b'"')) {
            return None;
        }
        if n == 0 {
            return Some(0);
        }
        let mut lower = [0u8; 2];
        for (i, l) in lower.iter_mut().enumerate().take(n) {
            *l = self.bytes[pos + i].to_ascii_lowercase();
        }
        let valid = match n {
            1 => matches!(lower[0], b'r' | b'u' | b'b' | b'f'),
            2 => matches!(&lower[..2], b"rb" | b"br" | b"fr" | b"rf"),
            _ => false,
        };
        valid.then_some(n)
    }

    fn string_prefix_is_f(prefix: &str) -> bool {
        prefix.bytes().any(|b| b == b'f' || b == b'F')
    }

    fn string_prefix_is_raw(prefix: &str) -> bool {
        prefix.bytes().any(|b| b == b'r' || b == b'R')
    }

    fn lex_string(&mut self) {
        let start = self.pos;
        let prefix_len = self.string_prefix_len().unwrap_or(0);
        let prefix = &self.src[self.pos..self.pos + prefix_len];
        let is_f = Self::string_prefix_is_f(prefix);
        let is_raw = Self::string_prefix_is_raw(prefix);
        self.pos += prefix_len;
        let quote = self.bytes[self.pos];
        let triple = self.bytes.get(self.pos + 1) == Some(&quote)
            && self.bytes.get(self.pos + 2) == Some(&quote);
        self.pos += if triple { 3 } else { 1 };

        if is_f {
            if !self.fstring_scan(quote, triple, is_raw) {
                self.push_error(self.span_from(start), "string sem fecho");
            }
            self.push_token(TokenKind::Str, start);
            return;
        }

        if !self.scan_plain_string_body(quote, triple) {
            self.push_error(self.span_from(start), "string sem fecho");
        }
        self.push_token(TokenKind::Str, start);
    }

    // Varre o corpo de uma string normal (sem 'f'), a partir de logo depois
    // da(s) aspa(s) de abertura. Devolve true se fechou corretamente, false
    // se chegou ao EOF (ou a uma newline não permitida) sem fechar — quem
    // chama é responsável por reportar o erro e emitir o token.
    fn scan_plain_string_body(&mut self, quote: u8, triple: bool) -> bool {
        loop {
            match self.bytes.get(self.pos) {
                None => return false,
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
                        return true;
                    }
                    if self.bytes.get(self.pos + 1) == Some(&quote)
                        && self.bytes.get(self.pos + 2) == Some(&quote)
                    {
                        self.pos += 3;
                        return true;
                    }
                    self.pos += 1;
                }
                Some(b'\n') | Some(b'\r') if !triple => return false,
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

    // --- f-strings (PEP 701) ---------------------------------------------
    //
    // Uma f-string é UM só token Str, do prefixo até à aspa de fecho. O
    // varrimento é feito à mão (não gera tokens internos): a parte literal
    // segue as mesmas regras de `scan_plain_string_body` (com `\`, e mais
    // `{{`/`}}` como escapes literais e um `}` sozinho como erro), e um `{`
    // não duplicado abre um campo de substituição, tratado por
    // `fstring_field`. Strings aninhadas (de qualquer prefixo válido,
    // incluindo a mesma aspa e f-strings aninhadas) são saltadas por
    // `skip_nested_string`. A profundidade de parênteses/colchetes/chavetas
    // dentro da expressão soma-se à pilha `open_brackets` (partilha o
    // limite do L8 e evita recursão ilimitada: `fstring_field` só recursa
    // por cada `{` real, que está sujeito a esse limite de 200).

    // Devolve true se a f-string fechou corretamente (aspa de fecho
    // encontrada), false se chegou ao EOF sem fechar.
    fn fstring_scan(&mut self, quote: u8, triple: bool, raw: bool) -> bool {
        loop {
            match self.fstring_literal(quote, triple, raw, /*in_format_spec*/ false) {
                FScan::Closed => return true,
                FScan::Eof => return false,
                FScan::FieldOpen => {
                    if !self.fstring_field(raw) {
                        return false;
                    }
                }
            }
        }
    }

    // Varre texto literal: ou a parte literal "de fora" de uma f-string
    // (termina na aspa de fecho), ou, quando `in_format_spec` é true, o
    // texto literal de um format spec (termina no '}' que fecha o campo).
    fn fstring_literal(
        &mut self,
        quote: u8,
        triple: bool,
        raw: bool,
        in_format_spec: bool,
    ) -> FScan {
        loop {
            match self.bytes.get(self.pos) {
                None => return FScan::Eof,
                Some(b'\\') if matches!(self.bytes.get(self.pos + 1), Some(b'{') | Some(b'}')) => {
                    self.pos += 1;
                }
                Some(b'\\')
                    if !raw
                        && self.bytes.get(self.pos + 1) == Some(&b'N')
                        && self.bytes.get(self.pos + 2) == Some(&b'{') =>
                {
                    self.pos += 3;
                    loop {
                        match self.bytes.get(self.pos) {
                            None => break,
                            Some(b'}') => {
                                self.pos += 1;
                                break;
                            }
                            Some(&b) if !in_format_spec && b == quote => {
                                if !triple {
                                    break;
                                }
                                if self.bytes.get(self.pos + 1) == Some(&quote)
                                    && self.bytes.get(self.pos + 2) == Some(&quote)
                                {
                                    break;
                                }
                                self.pos += 1;
                            }
                            Some(b'\n') | Some(b'\r') if !triple && !in_format_spec => break,
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
                Some(&b) if !in_format_spec && b == quote => {
                    if !triple {
                        self.pos += 1;
                        return FScan::Closed;
                    }
                    if self.bytes.get(self.pos + 1) == Some(&quote)
                        && self.bytes.get(self.pos + 2) == Some(&quote)
                    {
                        self.pos += 3;
                        return FScan::Closed;
                    }
                    self.pos += 1;
                }
                Some(b'\n') | Some(b'\r') if !triple && !in_format_spec => return FScan::Eof,
                Some(b'{') => {
                    if self.bytes.get(self.pos + 1) == Some(&b'{') {
                        self.pos += 2;
                        continue;
                    }
                    self.pos += 1;
                    return FScan::FieldOpen;
                }
                Some(b'}') => {
                    if in_format_spec {
                        // fecha o campo (o '{' correspondente já foi
                        // registado por quem chamou `fstring_field`).
                        self.pos += 1;
                        return FScan::Closed;
                    }
                    if self.bytes.get(self.pos + 1) == Some(&b'}') {
                        self.pos += 2;
                        continue;
                    }
                    self.push_error(
                        Span::new(self.file, self.pos as u32, self.pos as u32 + 1),
                        "'}' sozinho não é permitido numa f-string",
                    );
                    self.pos += 1;
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

    // Trata o conteúdo de um campo `{...}` de uma f-string, desde logo a
    // seguir ao '{' de abertura (já consumido por quem chamou) até
    // (incluindo) o '}' de fecho. Devolve false se chegou ao EOF antes do
    // fecho.
    fn fstring_field(&mut self, outer_raw: bool) -> bool {
        let open_pos = self.pos - 1;
        self.push_bracket_depth(b'{', open_pos);
        let mut depth = 0u32;
        let mut in_format_spec = false;
        loop {
            if !in_format_spec {
                self.skip_inline_whitespace();
            }
            match self.bytes.get(self.pos) {
                None => {
                    self.pop_bracket_depth();
                    return false;
                }
                Some(b'#') if !in_format_spec => {
                    self.skip_to_line_end();
                    continue;
                }
                Some(b'\n') | Some(b'\r') if !in_format_spec => {
                    self.consume_newline_bytes();
                    continue;
                }
                Some(b'\'') | Some(b'"') if !in_format_spec => {
                    if !self.skip_nested_string() {
                        self.pop_bracket_depth();
                        return false;
                    }
                    continue;
                }
                Some(&b) if !in_format_spec && is_ident_start(b) => {
                    if let Some(n) = self.string_prefix_len() {
                        self.pos += n;
                        continue;
                    }
                    self.skip_ident_run();
                    continue;
                }
                Some(b'\\') if !in_format_spec => {
                    let bs_pos = self.pos;
                    if matches!(self.bytes.get(self.pos + 1), Some(b'\n') | Some(b'\r')) {
                        self.pos += 1;
                    } else {
                        self.pos += 1;
                        self.push_error(
                            Span::new(self.file, bs_pos as u32, bs_pos as u32 + 1),
                            "carácter inesperado '\\'",
                        );
                    }
                }
                Some(b'(') | Some(b'[') if !in_format_spec => {
                    let ch = self.bytes[self.pos];
                    let p = self.pos;
                    self.pos += 1;
                    depth += 1;
                    self.push_bracket_depth(ch, p);
                }
                Some(b'{') if !in_format_spec => {
                    let p = self.pos;
                    self.pos += 1;
                    depth += 1;
                    self.push_bracket_depth(b'{', p);
                }
                Some(b')') | Some(b']') if !in_format_spec && depth > 0 => {
                    self.pos += 1;
                    depth -= 1;
                    self.pop_bracket_depth();
                }
                Some(b'}') if !in_format_spec && depth > 0 => {
                    self.pos += 1;
                    depth -= 1;
                    self.pop_bracket_depth();
                }
                Some(b'}') if !in_format_spec && depth == 0 => {
                    self.pos += 1;
                    self.pop_bracket_depth();
                    return true;
                }
                Some(b'!')
                    if !in_format_spec
                        && depth == 0
                        && self.bytes.get(self.pos + 1) != Some(&b'=') =>
                {
                    self.pos += 1;
                }
                Some(b':') if !in_format_spec && depth == 0 => {
                    self.pos += 1;
                    in_format_spec = true;
                }
                Some(_) if !in_format_spec => {
                    if let Some(c) = self.peek() {
                        self.pos += c.len_utf8();
                    } else {
                        self.pos += 1;
                    }
                }
                _ => {
                    // format spec: texto literal, com campos '{...}' aninhados.
                    match self.fstring_literal(0, false, outer_raw, true) {
                        FScan::Closed => {
                            self.pop_bracket_depth();
                            return true;
                        }
                        FScan::Eof => {
                            self.pop_bracket_depth();
                            return false;
                        }
                        FScan::FieldOpen => {
                            if !self.fstring_field(outer_raw) {
                                self.pop_bracket_depth();
                                return false;
                            }
                        }
                    }
                }
            }
        }
    }

    // Salta uma string (ou f-string) aninhada inteira, dentro da parte de
    // expressão de um campo — sem emitir tokens. Devolve false em EOF.
    fn skip_nested_string(&mut self) -> bool {
        let prefix_len = self.string_prefix_len().unwrap_or(0);
        let prefix = &self.src[self.pos..self.pos + prefix_len];
        let is_f = Self::string_prefix_is_f(prefix);
        let is_raw = Self::string_prefix_is_raw(prefix);
        self.pos += prefix_len;
        let Some(&quote) = self.bytes.get(self.pos) else {
            return false;
        };
        let triple = self.bytes.get(self.pos + 1) == Some(&quote)
            && self.bytes.get(self.pos + 2) == Some(&quote);
        self.pos += if triple { 3 } else { 1 };
        if is_f {
            self.fstring_scan(quote, triple, is_raw)
        } else {
            self.scan_plain_string_body(quote, triple)
        }
    }

    fn skip_ident_run(&mut self) {
        while let Some(c) = self.peek() {
            if c == '_' || c.is_alphanumeric() {
                self.pos += c.len_utf8();
            } else {
                break;
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
        let radix_name = match radix {
            b'x' => "hexadecimal",
            b'o' => "octal",
            _ => "binário",
        };
        let (saw_digit, ok) = self.scan_digit_run(is_digit);
        if !ok {
            self.push_error(
                self.span_from(start),
                "sublinhado inválido em literal numérico",
            );
            return;
        }
        if let Some(&b) = self.bytes.get(self.pos) {
            if b.is_ascii_digit() && !is_digit(b) {
                // dígito decimal que não é válido nesta base (ex.: '8' em 0o8).
                let bad_pos = self.pos;
                let ch = b as char;
                self.scan_digit_run(|b| b.is_ascii_digit());
                self.push_error(
                    Span::new(self.file, bad_pos as u32, bad_pos as u32 + 1),
                    format!("dígito inválido '{ch}' em literal {radix_name}"),
                );
                return;
            }
        }
        if let Some(c) = self.peek() {
            if c == '_' || c.is_alphabetic() {
                // letras coladas ao literal (ex.: '0x1g'): não é "falta um
                // espaço", é o literal inteiro que é inválido.
                let mut end = self.pos;
                for c in self.src[self.pos..].chars() {
                    if c == '_' || c.is_alphanumeric() {
                        end += c.len_utf8();
                    } else {
                        break;
                    }
                }
                self.pos = end;
                self.push_error(
                    self.span_from(start),
                    format!("literal {radix_name} inválido"),
                );
                return;
            }
        }
        if !saw_digit {
            self.push_error(
                self.span_from(start),
                format!("literal {radix_name} inválido: dígitos em falta"),
            );
            return;
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
