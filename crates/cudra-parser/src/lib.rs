mod expr;
mod stmt;

use cudra_ast::{Module, NodeId};
use cudra_diagnostics::Diagnostic;
use cudra_lexer::{Token, TokenKind};
use cudra_span::{FileId, Interner, Span, Symbol};

const MAX_DEPTH: u32 = 1000;

// Marcador de "a instrução atual falhou": o erro já foi registado no ponto
// onde foi detetado, e isto só serve para desviar o controlo até ao sítio
// que sabe recuperar (ver `recover_stmt`).
pub(crate) struct Bail;
pub(crate) type PResult<T> = Result<T, Bail>;

pub fn parse(
    file: FileId,
    src: &str,
    tokens: &[Token],
    interner: &mut Interner,
) -> (Module, Vec<Diagnostic>) {
    let mut p = Parser::new(file, src, tokens, interner);
    let module = p.parse_module();
    (module, p.diags)
}

struct Parser<'a> {
    #[allow(dead_code)]
    file: FileId,
    src: &'a str,
    toks: &'a [Token],
    pos: usize,
    interner: &'a mut Interner,
    diags: Vec<Diagnostic>,
    next_id: u32,
    rec_guard: u32,
}

impl<'a> Parser<'a> {
    fn new(file: FileId, src: &'a str, toks: &'a [Token], interner: &'a mut Interner) -> Self {
        Parser {
            file,
            src,
            toks,
            pos: 0,
            interner,
            diags: Vec::new(),
            next_id: 0,
            rec_guard: 0,
        }
    }

    fn parse_module(&mut self) -> Module {
        let mut items = Vec::new();
        while !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Dedent) {
                let sp = self.span();
                self.err(sp, "dedentação inesperada");
                self.bump();
                continue;
            }
            let before = self.pos;
            self.parse_top_item(&mut items);
            if self.pos == before {
                // salvaguarda: nunca deixar o parser preso.
                self.bump();
            }
        }
        Module { items }
    }

    // --- navegação de tokens -------------------------------------------

    fn kind(&self) -> TokenKind {
        self.toks[self.pos].kind
    }

    fn kind_at(&self, off: usize) -> TokenKind {
        self.toks
            .get(self.pos + off)
            .map(|t| t.kind)
            .unwrap_or(TokenKind::Eof)
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn at(&self, k: TokenKind) -> bool {
        self.kind() == k
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos];
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, k: TokenKind) -> Option<Token> {
        if self.at(k) {
            Some(self.bump())
        } else {
            None
        }
    }

    // Devolvido com o tempo de vida `'a` (o da fonte), não o de `&self`: dá
    // para chamar métodos com `&mut self` logo a seguir sem conflito de
    // empréstimos.
    fn text_of(&self, span: Span) -> &'a str {
        &self.src[span.lo as usize..span.hi as usize]
    }

    fn cur_text(&self) -> &'a str {
        self.text_of(self.span())
    }

    fn is_reserved(&self, text: &str) -> bool {
        self.at(TokenKind::Reserved) && self.cur_text() == text
    }

    fn next_id(&mut self) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        id
    }

    fn intern(&mut self, text: &str) -> Symbol {
        self.interner.intern(text)
    }

    fn mk_ident(&mut self, tok: Token) -> cudra_ast::Ident {
        let sym = self.intern(self.text_of(tok.span));
        cudra_ast::Ident {
            name: sym,
            span: tok.span,
        }
    }

    // --- erros -----------------------------------------------------------

    fn err(&mut self, span: Span, msg: impl Into<String>) {
        let msg = msg.into();
        self.diags
            .push(Diagnostic::error(msg.clone()).with_primary(span, msg));
    }

    fn bail(&mut self, span: Span, msg: impl Into<String>) -> Bail {
        self.err(span, msg);
        Bail
    }

    fn unsupported<T>(&mut self, span: Span, what: &str) -> PResult<T> {
        self.err(span, format!("{what} ainda não é suportado(a) na Fase 1"));
        Err(Bail)
    }

    fn expect(&mut self, k: TokenKind, what: &str) -> PResult<Token> {
        if let Some(t) = self.eat(k) {
            Ok(t)
        } else {
            let span = self.span();
            Err(self.bail(span, format!("esperava-se {what}")))
        }
    }

    fn expect_name(&mut self, what: &str) -> PResult<cudra_ast::Ident> {
        let tok = self.expect(TokenKind::Name, what)?;
        Ok(self.mk_ident(tok))
    }

    // --- profundidade das expressões -------------------------------------

    fn check_depth(&mut self, d: u32, span: Span) -> PResult<u32> {
        if d > MAX_DEPTH {
            Err(self.bail(span, "expressão demasiado aninhada (máximo 1000 níveis)"))
        } else {
            Ok(d)
        }
    }

    fn depth_bail<T>(&mut self) -> PResult<T> {
        let span = self.span();
        Err(self.bail(span, "expressão demasiado aninhada (máximo 1000 níveis)"))
    }

    // --- recuperação de erros ---------------------------------------------

    fn at_continuation_clause(&self) -> bool {
        matches!(self.kind(), TokenKind::Elif | TokenKind::Else)
            || (self.at(TokenKind::Reserved) && matches!(self.cur_text(), "except" | "finally"))
    }

    fn skip_to_line_end(&mut self) {
        while !matches!(
            self.kind(),
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
        ) {
            self.bump();
        }
        if self.at(TokenKind::Newline) {
            self.bump();
        }
    }

    fn skip_block_if_present(&mut self) {
        if !self.at(TokenKind::Indent) {
            return;
        }
        self.bump();
        let mut depth = 1u32;
        while depth > 0 {
            match self.kind() {
                TokenKind::Indent => {
                    depth += 1;
                    self.bump();
                }
                TokenKind::Dedent => {
                    depth -= 1;
                    self.bump();
                }
                TokenKind::Eof => break,
                _ => {
                    self.bump();
                }
            }
        }
    }

    // Recupera depois de uma instrução falhar: salta até ao fim da linha
    // lógica, o bloco indentado que se seguisse (se algum) e as cláusulas de
    // continuação (`elif`/`else`/`except`/`finally`) ao mesmo nível, cada
    // uma com o seu próprio bloco.
    fn recover_stmt(&mut self) {
        self.skip_to_line_end();
        self.skip_block_if_present();
        while self.at_continuation_clause() {
            self.skip_to_line_end();
            self.skip_block_if_present();
        }
    }
}
