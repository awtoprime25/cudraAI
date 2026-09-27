use cudra_ast::{BinOp, CmpOp, Expr, ExprKind, LogicOp, UnOp};
use cudra_lexer::TokenKind;
use cudra_span::Span;

use crate::{Bail, PResult, Parser, MAX_DEPTH};

impl<'a> Parser<'a> {
    fn mk_expr(&mut self, kind: ExprKind, span: Span) -> Expr {
        let id = self.next_id();
        Expr { id, kind, span }
    }

    pub(crate) fn parse_expr(&mut self) -> PResult<(Expr, u32)> {
        let (e, d) = self.parse_or()?;
        if self.at(TokenKind::If) {
            let sp = self.span();
            return self.unsupported(sp, "expressões condicionais ('a if c else b')");
        }
        Ok((e, d))
    }

    fn parse_or(&mut self) -> PResult<(Expr, u32)> {
        let (mut lhs, mut d) = self.parse_and()?;
        while self.at(TokenKind::Or) {
            self.bump();
            let (rhs, dr) = self.parse_and()?;
            let span = lhs.span.to(rhs.span);
            d = d.max(dr);
            lhs = self.mk_expr(
                ExprKind::Logical {
                    op: LogicOp::Or,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }
        Ok((lhs, d))
    }

    fn parse_and(&mut self) -> PResult<(Expr, u32)> {
        let (mut lhs, mut d) = self.parse_not_expr()?;
        while self.at(TokenKind::And) {
            self.bump();
            let (rhs, dr) = self.parse_not_expr()?;
            let span = lhs.span.to(rhs.span);
            d = d.max(dr);
            lhs = self.mk_expr(
                ExprKind::Logical {
                    op: LogicOp::And,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }
        Ok((lhs, d))
    }

    fn parse_not_expr(&mut self) -> PResult<(Expr, u32)> {
        if !self.at(TokenKind::Not) {
            return self.parse_compare();
        }
        self.rec_guard += 1;
        if self.rec_guard > MAX_DEPTH {
            self.rec_guard -= 1;
            return self.depth_bail();
        }
        let out = self.parse_not_body();
        self.rec_guard -= 1;
        out
    }

    fn parse_not_body(&mut self) -> PResult<(Expr, u32)> {
        let not_tok = self.bump();
        let (operand, d) = self.parse_not_expr()?;
        let span = not_tok.span.to(operand.span);
        let dd = self.check_depth(1 + d, span)?;
        Ok((
            self.mk_expr(
                ExprKind::Unary {
                    op: UnOp::Not,
                    operand: Box::new(operand),
                },
                span,
            ),
            dd,
        ))
    }

    fn parse_compare(&mut self) -> PResult<(Expr, u32)> {
        let (first, d0) = self.parse_bitor()?;
        let mut rest: Vec<(CmpOp, Expr)> = Vec::new();
        let mut maxd = d0;
        loop {
            let cmp_op = match self.kind() {
                TokenKind::Lt => Some(CmpOp::Lt),
                TokenKind::Le => Some(CmpOp::Le),
                TokenKind::Gt => Some(CmpOp::Gt),
                TokenKind::Ge => Some(CmpOp::Ge),
                TokenKind::EqEq => Some(CmpOp::Eq),
                TokenKind::NotEq => Some(CmpOp::Ne),
                _ => None,
            };
            if let Some(op) = cmp_op {
                self.bump();
                let (c, dc) = self.parse_bitor()?;
                maxd = maxd.max(dc);
                rest.push((op, c));
                continue;
            }
            if self.at(TokenKind::Not) && self.kind_at(1) == TokenKind::In {
                let sp = self.span();
                return self.unsupported(sp, "'not in'");
            }
            if self.at(TokenKind::In) {
                let sp = self.span();
                return self.unsupported(sp, "'in' numa comparação");
            }
            if self.is_reserved("is") {
                let sp = self.span();
                return self.unsupported(sp, "'is'");
            }
            break;
        }
        if rest.is_empty() {
            return Ok((first, d0));
        }
        let last_span = rest.last().map(|(_, e)| e.span).unwrap_or(first.span);
        let span = first.span.to(last_span);
        let d = self.check_depth(1 + maxd, span)?;
        Ok((
            self.mk_expr(
                ExprKind::Compare {
                    first: Box::new(first),
                    rest,
                },
                span,
            ),
            d,
        ))
    }

    fn parse_bitor(&mut self) -> PResult<(Expr, u32)> {
        let (mut lhs, mut d) = self.parse_bitxor()?;
        while self.at(TokenKind::Pipe) {
            self.bump();
            let (rhs, dr) = self.parse_bitxor()?;
            let span = lhs.span.to(rhs.span);
            d = d.max(dr);
            lhs = self.mk_binary(BinOp::BitOr, lhs, rhs, span);
        }
        Ok((lhs, d))
    }

    fn parse_bitxor(&mut self) -> PResult<(Expr, u32)> {
        let (mut lhs, mut d) = self.parse_bitand()?;
        while self.at(TokenKind::Caret) {
            self.bump();
            let (rhs, dr) = self.parse_bitand()?;
            let span = lhs.span.to(rhs.span);
            d = d.max(dr);
            lhs = self.mk_binary(BinOp::BitXor, lhs, rhs, span);
        }
        Ok((lhs, d))
    }

    fn parse_bitand(&mut self) -> PResult<(Expr, u32)> {
        let (mut lhs, mut d) = self.parse_shift()?;
        while self.at(TokenKind::Amp) {
            self.bump();
            let (rhs, dr) = self.parse_shift()?;
            let span = lhs.span.to(rhs.span);
            d = d.max(dr);
            lhs = self.mk_binary(BinOp::BitAnd, lhs, rhs, span);
        }
        Ok((lhs, d))
    }

    fn parse_shift(&mut self) -> PResult<(Expr, u32)> {
        let (mut lhs, mut d) = self.parse_addsub()?;
        loop {
            let op = match self.kind() {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.bump();
            let (rhs, dr) = self.parse_addsub()?;
            let span = lhs.span.to(rhs.span);
            d = d.max(dr);
            lhs = self.mk_binary(op, lhs, rhs, span);
        }
        Ok((lhs, d))
    }

    fn parse_addsub(&mut self) -> PResult<(Expr, u32)> {
        let (mut lhs, mut d) = self.parse_muldiv()?;
        loop {
            let op = match self.kind() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let (rhs, dr) = self.parse_muldiv()?;
            let span = lhs.span.to(rhs.span);
            d = d.max(dr);
            lhs = self.mk_binary(op, lhs, rhs, span);
        }
        Ok((lhs, d))
    }

    fn parse_muldiv(&mut self) -> PResult<(Expr, u32)> {
        let (mut lhs, mut d) = self.parse_factor()?;
        loop {
            if self.at(TokenKind::At) {
                let sp = self.span();
                return self.unsupported(sp, "'@' (multiplicação de matrizes)");
            }
            let op = match self.kind() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::DoubleSlash => BinOp::FloorDiv,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.bump();
            let (rhs, dr) = self.parse_factor()?;
            let span = lhs.span.to(rhs.span);
            d = d.max(dr);
            lhs = self.mk_binary(op, lhs, rhs, span);
        }
        Ok((lhs, d))
    }

    fn mk_binary(&mut self, op: BinOp, lhs: Expr, rhs: Expr, span: Span) -> Expr {
        self.mk_expr(
            ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            span,
        )
    }

    // `-`/`+`/`~` (nível "factor"); o operando de `**` também entra por
    // aqui. `rec_guard` sobe em cada chamada (quer consuma um prefixo, quer
    // só delegue no `**`), e é o que evita recursão sem limite em `-`×100000
    // ou `a**`×100000.
    fn parse_factor(&mut self) -> PResult<(Expr, u32)> {
        self.rec_guard += 1;
        if self.rec_guard > MAX_DEPTH {
            self.rec_guard -= 1;
            return self.depth_bail();
        }
        let out = self.parse_factor_body();
        self.rec_guard -= 1;
        out
    }

    fn parse_factor_body(&mut self) -> PResult<(Expr, u32)> {
        let op = match self.kind() {
            TokenKind::Minus => Some(UnOp::Neg),
            TokenKind::Plus => Some(UnOp::Pos),
            TokenKind::Tilde => Some(UnOp::Invert),
            _ => None,
        };
        let Some(op) = op else {
            return self.parse_power();
        };
        let op_tok = self.bump();
        let (operand, d) = self.parse_factor()?;
        let span = op_tok.span.to(operand.span);
        let dd = self.check_depth(1 + d, span)?;
        Ok((
            self.mk_expr(
                ExprKind::Unary {
                    op,
                    operand: Box::new(operand),
                },
                span,
            ),
            dd,
        ))
    }

    fn parse_power(&mut self) -> PResult<(Expr, u32)> {
        let (base, db) = self.parse_postfix()?;
        if !self.at(TokenKind::DoubleStar) {
            return Ok((base, db));
        }
        self.bump();
        let (rhs, dr) = self.parse_factor()?;
        let span = base.span.to(rhs.span);
        let d = self.check_depth(1 + db.max(dr), span)?;
        Ok((self.mk_binary(BinOp::Pow, base, rhs, span), d))
    }

    fn parse_postfix(&mut self) -> PResult<(Expr, u32)> {
        let (mut base, mut d) = self.parse_atom()?;
        if self.at(TokenKind::ColonEq) {
            let sp = self.span();
            return self.unsupported(sp, "o operador ':=' (walrus)");
        }
        loop {
            match self.kind() {
                TokenKind::LParen => {
                    self.bump();
                    let mut args = Vec::new();
                    let mut argd = 0u32;
                    if !self.at(TokenKind::RParen) {
                        loop {
                            if matches!(self.kind(), TokenKind::Star | TokenKind::DoubleStar) {
                                let sp = self.span();
                                return self.unsupported(sp, "argumentos com '*'/'**'");
                            }
                            if self.at(TokenKind::Name) && self.kind_at(1) == TokenKind::Eq {
                                let sp = self.span();
                                return self.unsupported(sp, "argumentos nomeados");
                            }
                            let (a, da) = self.parse_expr()?;
                            if self.at(TokenKind::For) {
                                let sp = self.span();
                                return self.unsupported(sp, "expressões geradoras");
                            }
                            argd = argd.max(da);
                            args.push(a);
                            if self.eat(TokenKind::Comma).is_some() {
                                if self.at(TokenKind::RParen) {
                                    break;
                                }
                                continue;
                            }
                            break;
                        }
                    }
                    let close = self.expect(TokenKind::RParen, "')'")?;
                    let span = base.span.to(close.span);
                    d = self.check_depth(1 + d.max(argd), span)?;
                    base = self.mk_expr(
                        ExprKind::Call {
                            callee: Box::new(base),
                            args,
                        },
                        span,
                    );
                }
                TokenKind::Dot => {
                    self.bump();
                    let name_tok = self.expect(TokenKind::Name, "um nome de atributo")?;
                    let attr = self.mk_ident(name_tok);
                    let span = base.span.to(name_tok.span);
                    d = self.check_depth(1 + d, span)?;
                    base = self.mk_expr(
                        ExprKind::Attr {
                            base: Box::new(base),
                            attr,
                        },
                        span,
                    );
                }
                TokenKind::LBracket => {
                    self.bump();
                    if self.at(TokenKind::Colon) {
                        let sp = self.span();
                        return self.unsupported(sp, "slices");
                    }
                    let (idx, di) = self.parse_expr()?;
                    if self.at(TokenKind::Colon) {
                        let sp = self.span();
                        return self.unsupported(sp, "slices");
                    }
                    if self.at(TokenKind::Comma) {
                        let sp = self.span();
                        return self.unsupported(sp, "índice com tuplo");
                    }
                    let close = self.expect(TokenKind::RBracket, "']'")?;
                    let span = base.span.to(close.span);
                    d = self.check_depth(1 + d.max(di), span)?;
                    base = self.mk_expr(
                        ExprKind::Index {
                            base: Box::new(base),
                            index: Box::new(idx),
                        },
                        span,
                    );
                }
                _ => break,
            }
        }
        Ok((base, d))
    }

    fn parse_paren_atom(&mut self) -> PResult<(Expr, u32)> {
        let open_span = self.span();
        self.bump();
        if self.at(TokenKind::RParen) {
            self.bump();
            return self.unsupported(open_span, "tuplos");
        }
        let (inner, d) = self.parse_expr()?;
        if self.at(TokenKind::Comma) {
            return self.unsupported(open_span, "tuplos");
        }
        if self.at(TokenKind::For) {
            return self.unsupported(open_span, "expressões geradoras");
        }
        self.expect(TokenKind::RParen, "')'")?;
        Ok((inner, d))
    }

    fn parse_atom(&mut self) -> PResult<(Expr, u32)> {
        let span = self.span();
        match self.kind() {
            TokenKind::Int => {
                self.bump();
                self.lit_int(span)
            }
            TokenKind::Float => {
                self.bump();
                self.lit_float(span)
            }
            TokenKind::True => {
                self.bump();
                Ok((self.mk_expr(ExprKind::Bool(true), span), 1))
            }
            TokenKind::False => {
                self.bump();
                Ok((self.mk_expr(ExprKind::Bool(false), span), 1))
            }
            TokenKind::NoneKw => {
                self.bump();
                Ok((self.mk_expr(ExprKind::NoneLit, span), 1))
            }
            TokenKind::Name => {
                let tok = self.bump();
                let ident = self.mk_ident(tok);
                Ok((self.mk_expr(ExprKind::Name(ident), span), 1))
            }
            TokenKind::LParen => self.parse_paren_atom(),
            TokenKind::Str => {
                self.bump();
                self.unsupported(span, "strings")
            }
            TokenKind::Ellipsis => {
                self.bump();
                self.unsupported(span, "'...'")
            }
            TokenKind::LBracket => {
                self.bump();
                self.unsupported(span, "listas")
            }
            TokenKind::LBrace => {
                self.bump();
                self.unsupported(span, "dicionários/conjuntos")
            }
            TokenKind::Star => {
                self.bump();
                self.unsupported(span, "'*' em expressões")
            }
            TokenKind::ColonEq => {
                self.bump();
                self.unsupported(span, "operador ':='")
            }
            TokenKind::Reserved if self.cur_text() == "lambda" => {
                self.bump();
                self.unsupported(span, "'lambda'")
            }
            TokenKind::Reserved if self.cur_text() == "yield" => {
                self.bump();
                self.unsupported(span, "'yield'")
            }
            TokenKind::Reserved if self.cur_text() == "await" => {
                self.bump();
                self.unsupported(span, "'await'")
            }
            _ => Err(self.bail(span, "esperava-se uma expressão")),
        }
    }

    fn lit_int(&mut self, span: Span) -> PResult<(Expr, u32)> {
        let text = self.text_of(span);
        let cleaned: String = text.chars().filter(|&c| c != '_').collect();
        let bytes = cleaned.as_bytes();
        let (digits, radix): (&str, u32) = if bytes.len() > 1 && bytes[0] == b'0' {
            match bytes[1] {
                b'x' | b'X' => (&cleaned[2..], 16),
                b'o' | b'O' => (&cleaned[2..], 8),
                b'b' | b'B' => (&cleaned[2..], 2),
                _ => (&cleaned[..], 10),
            }
        } else {
            (&cleaned[..], 10)
        };
        if radix == 10
            && digits.len() > 1
            && digits.as_bytes()[0] == b'0'
            && digits.bytes().any(|b| b != b'0')
        {
            self.err(
                span,
                "zeros à esquerda em literais inteiros decimais ainda não são suportados",
            );
            return Err(Bail);
        }
        match u64::from_str_radix(digits, radix) {
            Ok(v) => Ok((self.mk_expr(ExprKind::Int(v), span), 1)),
            Err(_) => {
                self.err(
                    span,
                    "inteiros maiores do que 18446744073709551615 ainda não são suportados",
                );
                Err(Bail)
            }
        }
    }

    fn lit_float(&mut self, span: Span) -> PResult<(Expr, u32)> {
        let text = self.text_of(span);
        let cleaned: String = text.chars().filter(|&c| c != '_').collect();
        match cleaned.parse::<f64>() {
            Ok(v) => Ok((self.mk_expr(ExprKind::Float(v), span), 1)),
            Err(_) => Err(self.bail(span, "número em vírgula flutuante inválido")),
        }
    }
}
