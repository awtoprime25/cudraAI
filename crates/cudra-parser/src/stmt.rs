use cudra_ast::{
    Block, Expr, ExprKind, FuncDef, Item, Param, Stmt, StmtKind, Target, TypeExpr, TypeExprKind,
};
use cudra_lexer::TokenKind;

use crate::{Bail, PResult, Parser};

impl<'a> Parser<'a> {
    // --- topo -------------------------------------------------------------

    pub(crate) fn parse_top_item(&mut self, items: &mut Vec<Item>) {
        if matches!(self.kind(), TokenKind::At | TokenKind::Def) {
            match self.parse_func_def() {
                Ok(f) => items.push(Item::Func(f)),
                Err(Bail) => self.recover_stmt(),
            }
            return;
        }
        let mut stmts = Vec::new();
        self.stmt_one(&mut stmts);
        for s in stmts {
            items.push(Item::Stmt(s));
        }
    }

    // Uma posição de instrução: se e caso for de topo já foi tratado por
    // `parse_top_item`, o que resta aqui cobre tudo o resto — usado no topo
    // (para instruções que não são `def`) e dentro de qualquer bloco.
    fn stmt_one(&mut self, out: &mut Vec<Stmt>) {
        match self.kind() {
            TokenKind::If => match self.parse_if() {
                Ok(s) => out.push(s),
                Err(Bail) => self.recover_stmt(),
            },
            TokenKind::While => match self.parse_while() {
                Ok(Some(s)) => out.push(s),
                Ok(None) => {}
                Err(Bail) => self.recover_stmt(),
            },
            TokenKind::For => match self.parse_for() {
                Ok(Some(s)) => out.push(s),
                Ok(None) => {}
                Err(Bail) => self.recover_stmt(),
            },
            TokenKind::At | TokenKind::Def => {
                let span = self.span();
                let _: PResult<Stmt> =
                    self.unsupported(span, "definição de função aninhada ainda não é suportada");
                self.recover_stmt();
            }
            TokenKind::Reserved => {
                let text = self.cur_text();
                let msg = format!("'{text}' ainda não é suportado");
                let span = self.span();
                let _: PResult<Stmt> = self.unsupported(span, &msg);
                self.recover_stmt();
            }
            TokenKind::Name if self.at_match_stmt() => {
                let span = self.span();
                let _: PResult<Stmt> = self.unsupported(span, "'match' ainda não é suportado");
                self.recover_stmt();
            }
            TokenKind::Name if self.at_type_alias() => {
                let span = self.span();
                let _: PResult<Stmt> = self.unsupported(span, "'type' ainda não é suportado");
                self.recover_stmt();
            }
            _ => self.parse_simple_stmt_line(out),
        }
    }

    // Tokens que só podem aparecer a seguir a uma anotação de tipo se esta
    // continuar como uma expressão geral (fora do subconjunto de tipos).
    fn at_expr_operator(&self) -> bool {
        matches!(
            self.kind(),
            TokenKind::Or
                | TokenKind::And
                | TokenKind::Not
                | TokenKind::Lt
                | TokenKind::Le
                | TokenKind::Gt
                | TokenKind::Ge
                | TokenKind::EqEq
                | TokenKind::NotEq
                | TokenKind::In
                | TokenKind::Pipe
                | TokenKind::Caret
                | TokenKind::Amp
                | TokenKind::Shl
                | TokenKind::Shr
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::DoubleSlash
                | TokenKind::Percent
                | TokenKind::DoubleStar
                | TokenKind::If
        ) || self.is_reserved("is")
    }

    fn at_match_stmt(&self) -> bool {
        self.at(TokenKind::Name) && self.cur_text() == "match" && self.line_ends_with_colon()
    }

    fn at_type_alias(&self) -> bool {
        self.at(TokenKind::Name)
            && self.cur_text() == "type"
            && self.kind_at(1) == TokenKind::Name
            && matches!(self.kind_at(2), TokenKind::Eq | TokenKind::LBracket)
    }

    fn line_ends_with_colon(&self) -> bool {
        let mut i = self.pos;
        let mut last = None;
        loop {
            match self.toks.get(i).map(|t| t.kind) {
                Some(TokenKind::Newline) | Some(TokenKind::Eof) | None => break,
                Some(k) => {
                    last = Some(k);
                    i += 1;
                }
            }
        }
        matches!(last, Some(TokenKind::Colon))
    }

    // --- blocos -------------------------------------------------------------

    fn parse_block(&mut self) -> PResult<Block> {
        self.expect(TokenKind::Colon, "':'")?;
        let start = self.prev_span();
        if self.eat(TokenKind::Newline).is_some() {
            self.expect(TokenKind::Indent, "um bloco indentado")?;
            let mut stmts = Vec::with_capacity(1);
            while !matches!(self.kind(), TokenKind::Dedent | TokenKind::Eof) {
                let before = self.pos;
                self.stmt_one(&mut stmts);
                if self.pos == before {
                    // salvaguarda: nunca deixar o parser preso.
                    self.bump();
                }
            }
            let end = self.span();
            self.expect(TokenKind::Dedent, "o fim do bloco")?;
            let span = start.to(end);
            Ok(Block { stmts, span })
        } else {
            let mut stmts = Vec::with_capacity(1);
            self.parse_simple_stmt_line(&mut stmts);
            let span = match (stmts.first(), stmts.last()) {
                (Some(f), Some(l)) => f.span.to(l.span),
                _ => start,
            };
            Ok(Block { stmts, span })
        }
    }

    // --- instruções simples -------------------------------------------------

    // Nunca falha: cada instrução simples que rebenta é recuperada por si
    // (salta até ao próximo ';'/fim de linha), e as restantes na mesma
    // linha continuam a ser parseadas.
    fn parse_simple_stmt_line(&mut self, out: &mut Vec<Stmt>) {
        loop {
            match self.parse_simple_stmt() {
                Ok(s) => out.push(s),
                Err(Bail) => {
                    while !matches!(
                        self.kind(),
                        TokenKind::Semi | TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
                    ) {
                        self.bump();
                    }
                }
            }
            if self.eat(TokenKind::Semi).is_some() {
                if matches!(
                    self.kind(),
                    TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
                ) {
                    break;
                }
                continue;
            }
            break;
        }
        if !matches!(
            self.kind(),
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
        ) {
            let sp = self.span();
            self.err(sp, "esperava-se o fim da instrução");
            while !matches!(
                self.kind(),
                TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof
            ) {
                self.bump();
            }
        }
        if self.at(TokenKind::Newline) {
            self.bump();
        }
    }

    fn parse_simple_stmt(&mut self) -> PResult<Stmt> {
        match self.kind() {
            TokenKind::Return => self.parse_return(),
            TokenKind::Break => {
                let sp = self.span();
                self.bump();
                let id = self.next_id();
                Ok(Stmt {
                    id,
                    kind: StmtKind::Break,
                    span: sp,
                })
            }
            TokenKind::Continue => {
                let sp = self.span();
                self.bump();
                let id = self.next_id();
                Ok(Stmt {
                    id,
                    kind: StmtKind::Continue,
                    span: sp,
                })
            }
            TokenKind::Pass => {
                let sp = self.span();
                self.bump();
                let id = self.next_id();
                Ok(Stmt {
                    id,
                    kind: StmtKind::Pass,
                    span: sp,
                })
            }
            TokenKind::Import => self.parse_import(),
            TokenKind::From => self.parse_from_import(),
            _ => self.parse_assign_or_expr_stmt(),
        }
    }

    fn parse_return(&mut self) -> PResult<Stmt> {
        let start = self.span();
        self.bump();
        if matches!(
            self.kind(),
            TokenKind::Newline | TokenKind::Semi | TokenKind::Dedent | TokenKind::Eof
        ) {
            let id = self.next_id();
            return Ok(Stmt {
                id,
                kind: StmtKind::Return(None),
                span: start,
            });
        }
        let (e, _d) = self.parse_expr()?;
        if self.at(TokenKind::Comma) {
            let sp = self.span();
            return self.unsupported(sp, "tuplos ainda não são suportados");
        }
        let span = start.to(e.span);
        let id = self.next_id();
        Ok(Stmt {
            id,
            kind: StmtKind::Return(Some(e)),
            span,
        })
    }

    fn parse_import(&mut self) -> PResult<Stmt> {
        let start = self.span();
        self.bump();
        let name_tok = self.expect(TokenKind::Name, "o nome do módulo")?;
        if self.at(TokenKind::Dot) {
            let sp = self.span();
            return self.unsupported(sp, "'import' com módulo composto ainda não é suportado");
        }
        if self.is_reserved("as") {
            let sp = self.span();
            return self.unsupported(sp, "'import ... as' ainda não é suportado");
        }
        if self.at(TokenKind::Comma) {
            let sp = self.span();
            return self.unsupported(sp, "'import' com vários módulos ainda não é suportado");
        }
        let module = self.mk_ident(name_tok);
        let span = start.to(name_tok.span);
        let id = self.next_id();
        Ok(Stmt {
            id,
            kind: StmtKind::Import { module },
            span,
        })
    }

    fn parse_from_import(&mut self) -> PResult<Stmt> {
        let start = self.span();
        self.bump();
        if self.at(TokenKind::Dot) {
            let sp = self.span();
            return self.unsupported(sp, "import relativo ainda não é suportado");
        }
        let mod_tok = self.expect(TokenKind::Name, "o nome do módulo")?;
        if self.at(TokenKind::Dot) {
            let sp = self.span();
            return self.unsupported(sp, "'import' com módulo composto ainda não é suportado");
        }
        let module = self.mk_ident(mod_tok);
        self.expect(TokenKind::Import, "'import'")?;
        if self.at(TokenKind::Star) {
            let sp = self.span();
            return self.unsupported(sp, "'from ... import *' ainda não é suportado");
        }
        let paren = self.eat(TokenKind::LParen).is_some();
        if paren && self.at(TokenKind::RParen) {
            let sp = self.span();
            return Err(self.bail(sp, "esperava-se pelo menos um nome importado"));
        }
        let mut names = Vec::new();
        loop {
            let n_tok = self.expect(TokenKind::Name, "um nome importado")?;
            if self.is_reserved("as") {
                let sp = self.span();
                return self.unsupported(sp, "'import ... as' ainda não é suportado");
            }
            names.push(self.mk_ident(n_tok));
            if self.eat(TokenKind::Comma).is_some() {
                if paren && self.at(TokenKind::RParen) {
                    break;
                }
                continue;
            }
            break;
        }
        if paren {
            self.expect(TokenKind::RParen, "')'")?;
        }
        let end = self.prev_span();
        let span = start.to(end);
        let id = self.next_id();
        Ok(Stmt {
            id,
            kind: StmtKind::FromImport { module, names },
            span,
        })
    }

    fn parse_assign_or_expr_stmt(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let (first_expr, _d) = self.parse_expr()?;

        if self.at(TokenKind::AtEq) {
            let sp = self.span();
            return self.unsupported(sp, "'@=' ainda não é suportado");
        }

        if let Some(aug_op) = self.aug_op_kind() {
            let target = self.expr_to_target(first_expr)?;
            self.bump();
            let (value, _dv) = self.parse_expr()?;
            if self.at(TokenKind::Comma) {
                let sp = self.span();
                return self.unsupported(sp, "tuplos ainda não são suportados");
            }
            let span = start.to(value.span);
            let id = self.next_id();
            return Ok(Stmt {
                id,
                kind: StmtKind::AugAssign {
                    target,
                    op: aug_op,
                    value,
                },
                span,
            });
        }

        if self.at(TokenKind::Colon) {
            let target = self.expr_to_target_for_annotation(first_expr)?;
            self.bump();
            let ty = self.parse_type_expr()?;
            if self.eat(TokenKind::Eq).is_none() {
                let sp = self.span();
                return self.unsupported(sp, "anotação sem valor ainda não é suportada");
            }
            let (value, _dv) = self.parse_expr()?;
            if self.at(TokenKind::Eq) {
                let sp = self.span();
                return Err(self.bail(sp, "não se pode encadear uma atribuição anotada"));
            }
            if self.at(TokenKind::Comma) {
                let sp = self.span();
                return self.unsupported(sp, "tuplos ainda não são suportados");
            }
            let span = start.to(value.span);
            let id = self.next_id();
            return Ok(Stmt {
                id,
                kind: StmtKind::Assign {
                    target,
                    ty: Some(ty),
                    value,
                },
                span,
            });
        }

        if self.at(TokenKind::Eq) {
            self.bump();
            let target = self.expr_to_target(first_expr)?;
            let (value, _dv) = self.parse_expr()?;
            if self.at(TokenKind::Eq) {
                let sp = self.span();
                return self.unsupported(sp, "atribuição múltipla ainda não é suportada");
            }
            if self.at(TokenKind::Comma) {
                let sp = self.span();
                return self.unsupported(sp, "tuplos ainda não são suportados");
            }
            let span = start.to(value.span);
            let id = self.next_id();
            return Ok(Stmt {
                id,
                kind: StmtKind::Assign {
                    target,
                    ty: None,
                    value,
                },
                span,
            });
        }

        if self.at(TokenKind::Comma) {
            let sp = self.span();
            return self.unsupported(sp, "tuplos ainda não são suportados");
        }

        let span = first_expr.span;
        let id = self.next_id();
        Ok(Stmt {
            id,
            kind: StmtKind::Expr(first_expr),
            span,
        })
    }

    fn aug_op_kind(&self) -> Option<cudra_ast::BinOp> {
        use cudra_ast::BinOp::*;
        match self.kind() {
            TokenKind::PlusEq => Some(Add),
            TokenKind::MinusEq => Some(Sub),
            TokenKind::StarEq => Some(Mul),
            TokenKind::SlashEq => Some(Div),
            TokenKind::DoubleSlashEq => Some(FloorDiv),
            TokenKind::PercentEq => Some(Mod),
            TokenKind::DoubleStarEq => Some(Pow),
            TokenKind::AmpEq => Some(BitAnd),
            TokenKind::PipeEq => Some(BitOr),
            TokenKind::CaretEq => Some(BitXor),
            TokenKind::ShlEq => Some(Shl),
            TokenKind::ShrEq => Some(Shr),
            _ => None,
        }
    }

    fn expr_to_target(&mut self, e: Expr) -> PResult<Target> {
        match e.kind {
            ExprKind::Name(ident) => Ok(Target::Name(ident)),
            ExprKind::Index { base, index } => Ok(Target::Index { base, index }),
            ExprKind::Attr { .. } => self.unsupported(
                e.span,
                "atributo como alvo de atribuição ainda não é suportado",
            ),
            _ => Err(self.bail(e.span, "alvo de atribuição inválido")),
        }
    }

    fn expr_to_target_for_annotation(&mut self, e: Expr) -> PResult<Target> {
        match e.kind {
            ExprKind::Name(ident) => Ok(Target::Name(ident)),
            ExprKind::Attr { .. } => {
                self.unsupported(e.span, "anotação em atributo ainda não é suportada")
            }
            ExprKind::Index { .. } => {
                self.unsupported(e.span, "anotação em índice ainda não é suportada")
            }
            _ => Err(self.bail(e.span, "alvo de atribuição inválido")),
        }
    }

    // --- if/while/for -------------------------------------------------------

    fn parse_if(&mut self) -> PResult<Stmt> {
        let start = self.span();
        self.bump();
        let (cond, _d) = self.parse_expr()?;
        let then = self.parse_block()?;
        let mut elifs = Vec::new();
        while self.at(TokenKind::Elif) {
            self.bump();
            let (c, _d) = self.parse_expr()?;
            let b = self.parse_block()?;
            elifs.push((c, b));
        }
        let orelse = if self.at(TokenKind::Else) {
            self.bump();
            Some(self.parse_block()?)
        } else {
            None
        };
        let id = self.next_id();
        let end = orelse
            .as_ref()
            .map(|b| b.span)
            .or_else(|| elifs.last().map(|(_, b)| b.span))
            .unwrap_or(then.span);
        let span = start.to(end);
        Ok(Stmt {
            id,
            kind: StmtKind::If {
                cond,
                then,
                elifs,
                orelse,
            },
            span,
        })
    }

    fn parse_while(&mut self) -> PResult<Option<Stmt>> {
        let start = self.span();
        self.bump();
        let (cond, _d) = self.parse_expr()?;
        let body = self.parse_block()?;
        if self.at(TokenKind::Else) {
            let else_span = self.span();
            self.unsup(else_span, "'while ... else' ainda não é suportado");
            self.bump();
            if self.parse_block().is_err() {
                self.recover_stmt();
            }
            return Ok(None);
        }
        let id = self.next_id();
        let span = start.to(body.span);
        Ok(Some(Stmt {
            id,
            kind: StmtKind::While { cond, body },
            span,
        }))
    }

    fn parse_for(&mut self) -> PResult<Option<Stmt>> {
        let start = self.span();
        self.bump();
        let var = self.parse_for_target()?;
        self.expect(TokenKind::In, "'in'")?;
        let (iter, _d) = self.parse_expr()?;
        if self.at(TokenKind::Comma) {
            let sp = self.span();
            return self.unsupported(sp, "tuplos ainda não são suportados");
        }
        let body = self.parse_block()?;
        if self.at(TokenKind::Else) {
            let else_span = self.span();
            self.unsup(else_span, "'for ... else' ainda não é suportado");
            self.bump();
            if self.parse_block().is_err() {
                self.recover_stmt();
            }
            return Ok(None);
        }
        let id = self.next_id();
        let span = start.to(body.span);
        Ok(Some(Stmt {
            id,
            kind: StmtKind::For { var, iter, body },
            span,
        }))
    }

    fn parse_for_target(&mut self) -> PResult<cudra_ast::Ident> {
        let mut parens = 0u32;
        while self.eat(TokenKind::LParen).is_some() {
            parens += 1;
        }
        let ident = self.expect_name("o nome da variável do ciclo")?;
        if self.at(TokenKind::Comma) {
            let sp = self.span();
            return self.unsupported(sp, "tuplos como alvo do 'for' ainda não são suportados");
        }
        while parens > 0 {
            self.expect(TokenKind::RParen, "')'")?;
            parens -= 1;
        }
        Ok(ident)
    }

    // --- funções e tipos ------------------------------------------------------

    fn parse_decorators(&mut self) -> PResult<Vec<Expr>> {
        let mut decos = Vec::new();
        while self.at(TokenKind::At) {
            self.bump();
            let (e, _d) = self.parse_expr()?;
            decos.push(e);
            self.expect(TokenKind::Newline, "o fim de linha depois do decorador")?;
        }
        Ok(decos)
    }

    fn parse_func_def(&mut self) -> PResult<FuncDef> {
        let start = self.span();
        let decorators = self.parse_decorators()?;
        if !self.at(TokenKind::Def) {
            let sp = self.span();
            return self.unsupported(sp, "declaração decorada ainda não é suportada");
        }
        self.bump();
        let name = self.expect_name("o nome da função")?;
        if self.at(TokenKind::LBracket) {
            let sp = self.span();
            return self.unsupported(
                sp,
                "parâmetros de tipo (genéricos) ainda não são suportados",
            );
        }
        self.expect(TokenKind::LParen, "'('")?;
        let params = self.parse_params()?;
        self.expect(TokenKind::RParen, "')'")?;
        let ret = if self.eat(TokenKind::Arrow).is_some() {
            Some(self.parse_type_expr()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        let id = self.next_id();
        let span = start.to(body.span);
        Ok(FuncDef {
            id,
            name,
            decorators,
            params,
            ret,
            body,
            span,
        })
    }

    fn parse_params(&mut self) -> PResult<Vec<Param>> {
        if self.at(TokenKind::RParen) {
            return Ok(Vec::new());
        }
        // `Vec::new()` daria capacidade para 4 parâmetros; quase todas as
        // funções têm um ou dois.
        let mut params = Vec::with_capacity(1);
        loop {
            if matches!(self.kind(), TokenKind::Star | TokenKind::DoubleStar) {
                let sp = self.span();
                return self.unsupported(sp, "'*'/'**' em parâmetros ainda não são suportados");
            }
            if self.at(TokenKind::Slash) {
                let sp = self.span();
                return self.unsupported(sp, "parâmetros só-posicionais ainda não são suportados");
            }
            let start = self.span();
            let name_tok = self.expect(TokenKind::Name, "o nome do parâmetro")?;
            let name = self.mk_ident(name_tok);
            let ty = if self.eat(TokenKind::Colon).is_some() {
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            if self.at(TokenKind::Eq) {
                let sp = self.span();
                return self.unsupported(
                    sp,
                    "parâmetros com valor por omissão ainda não são suportados",
                );
            }
            let end = ty.as_ref().map(|t| t.span).unwrap_or(name.span);
            let span = start.to(end);
            let id = self.next_id();
            params.push(Param { id, name, ty, span });
            if self.eat(TokenKind::Comma).is_some() {
                if self.at(TokenKind::RParen) {
                    break;
                }
                continue;
            }
            break;
        }
        Ok(params)
    }

    fn parse_type_expr(&mut self) -> PResult<TypeExpr> {
        let start = self.span();
        if self.at(TokenKind::NoneKw) {
            self.bump();
            let sym = self.intern("None");
            let ident = cudra_ast::Ident {
                name: sym,
                span: start,
            };
            if matches!(self.kind(), TokenKind::Dot | TokenKind::LBracket)
                || self.at_expr_operator()
            {
                let sp = self.span();
                return self.unsupported(sp, "anotação de tipo ainda não é suportada");
            }
            let id = self.next_id();
            return Ok(TypeExpr {
                id,
                kind: TypeExprKind::Name(ident),
                span: start,
            });
        }
        if !self.at(TokenKind::Name) {
            let sp = self.span();
            return self.unsupported(sp, "anotação de tipo ainda não é suportada");
        }
        let name_tok = self.bump();
        let ident = self.mk_ident(name_tok);
        let (kind, end_span) = if self.eat(TokenKind::LBracket).is_some() {
            let mut args = Vec::new();
            loop {
                args.push(self.parse_type_expr()?);
                if self.eat(TokenKind::Comma).is_some() {
                    if self.at(TokenKind::RBracket) {
                        break;
                    }
                    continue;
                }
                break;
            }
            let close = self.expect(TokenKind::RBracket, "']'")?;
            (TypeExprKind::Generic { base: ident, args }, close.span)
        } else {
            (TypeExprKind::Name(ident), name_tok.span)
        };
        if matches!(self.kind(), TokenKind::Dot | TokenKind::LBracket) || self.at_expr_operator() {
            let sp = self.span();
            return self.unsupported(sp, "anotação de tipo ainda não é suportada");
        }
        let id = self.next_id();
        Ok(TypeExpr {
            id,
            kind,
            span: start.to(end_span),
        })
    }
}
