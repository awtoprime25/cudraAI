use cudra_span::Span;
use logos::Logos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Int,
    Float,
    Name,
    True,
    False,
    NoneKw,
    Def,
    Return,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Pass,
    And,
    Or,
    Not,
    Import,
    From,
    Reserved,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Dot,
    Arrow,
    At,
    Plus,
    Minus,
    Star,
    Slash,
    DoubleSlash,
    Percent,
    DoubleStar,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    NotEq,
    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    DoubleSlashEq,
    PercentEq,
    DoubleStarEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
    AtEq,
    ColonEq,
    Ellipsis,
    Semi,
    LBrace,
    RBrace,
    Str,
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Copy, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

// Palavras reservadas do Python que a Fase 1 não suporta: token próprio para
// dar um erro claro mais tarde (parser/sema), em vez de "identificador".
pub(crate) fn classify_ident(text: &str) -> TokenKind {
    match text {
        "True" => TokenKind::True,
        "False" => TokenKind::False,
        "None" => TokenKind::NoneKw,
        "def" => TokenKind::Def,
        "return" => TokenKind::Return,
        "if" => TokenKind::If,
        "elif" => TokenKind::Elif,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "for" => TokenKind::For,
        "in" => TokenKind::In,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "pass" => TokenKind::Pass,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "import" => TokenKind::Import,
        "from" => TokenKind::From,
        "class" | "lambda" | "global" | "nonlocal" | "try" | "except" | "finally" | "raise"
        | "with" | "as" | "yield" | "async" | "await" | "del" | "assert" | "is" => {
            TokenKind::Reserved
        }
        _ => TokenKind::Name,
    }
}

// Sublexer usado só para a parte "mecânica": identificadores ASCII e
// operadores/pontuação. Números e strings têm regras próprias de mais para
// caber num regex do logos e são reconhecidos à mão em `lib.rs`; NEWLINE,
// INDENT, DEDENT e EOF não existem a este nível.
#[derive(Logos, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    #[regex("[A-Za-z_][A-Za-z0-9_]*")]
    Ident,

    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(":=")]
    ColonEq,
    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("->")]
    Arrow,
    #[token("...")]
    Ellipsis,
    #[token(".")]
    Dot,
    #[token("@=")]
    AtEq,
    #[token("@")]
    At,
    #[token("+=")]
    PlusEq,
    #[token("+")]
    Plus,
    #[token("-=")]
    MinusEq,
    #[token("-")]
    Minus,
    #[token("**=")]
    DoubleStarEq,
    #[token("**")]
    DoubleStar,
    #[token("*=")]
    StarEq,
    #[token("*")]
    Star,
    #[token("//=")]
    DoubleSlashEq,
    #[token("//")]
    DoubleSlash,
    #[token("/=")]
    SlashEq,
    #[token("/")]
    Slash,
    #[token("%=")]
    PercentEq,
    #[token("%")]
    Percent,
    #[token("&=")]
    AmpEq,
    #[token("&")]
    Amp,
    #[token("|=")]
    PipeEq,
    #[token("|")]
    Pipe,
    #[token("^=")]
    CaretEq,
    #[token("^")]
    Caret,
    #[token("~")]
    Tilde,
    #[token("<<=")]
    ShlEq,
    #[token("<<")]
    Shl,
    #[token("<=")]
    Le,
    #[token("<")]
    Lt,
    #[token(">>=")]
    ShrEq,
    #[token(">>")]
    Shr,
    #[token(">=")]
    Ge,
    #[token(">")]
    Gt,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("=")]
    Eq,
}

impl Base {
    pub(crate) fn to_token_kind(self) -> TokenKind {
        match self {
            Base::Ident => TokenKind::Name, // nunca chamado: Ident é sempre classificado à parte
            Base::LParen => TokenKind::LParen,
            Base::RParen => TokenKind::RParen,
            Base::LBracket => TokenKind::LBracket,
            Base::RBracket => TokenKind::RBracket,
            Base::Colon => TokenKind::Colon,
            Base::ColonEq => TokenKind::ColonEq,
            Base::Comma => TokenKind::Comma,
            Base::Semi => TokenKind::Semi,
            Base::LBrace => TokenKind::LBrace,
            Base::RBrace => TokenKind::RBrace,
            Base::Arrow => TokenKind::Arrow,
            Base::Ellipsis => TokenKind::Ellipsis,
            Base::Dot => TokenKind::Dot,
            Base::At => TokenKind::At,
            Base::AtEq => TokenKind::AtEq,
            Base::Plus => TokenKind::Plus,
            Base::PlusEq => TokenKind::PlusEq,
            Base::Minus => TokenKind::Minus,
            Base::MinusEq => TokenKind::MinusEq,
            Base::Star => TokenKind::Star,
            Base::StarEq => TokenKind::StarEq,
            Base::DoubleStar => TokenKind::DoubleStar,
            Base::DoubleStarEq => TokenKind::DoubleStarEq,
            Base::Slash => TokenKind::Slash,
            Base::SlashEq => TokenKind::SlashEq,
            Base::DoubleSlash => TokenKind::DoubleSlash,
            Base::DoubleSlashEq => TokenKind::DoubleSlashEq,
            Base::Percent => TokenKind::Percent,
            Base::PercentEq => TokenKind::PercentEq,
            Base::Amp => TokenKind::Amp,
            Base::AmpEq => TokenKind::AmpEq,
            Base::Pipe => TokenKind::Pipe,
            Base::PipeEq => TokenKind::PipeEq,
            Base::Caret => TokenKind::Caret,
            Base::CaretEq => TokenKind::CaretEq,
            Base::Tilde => TokenKind::Tilde,
            Base::Shl => TokenKind::Shl,
            Base::ShlEq => TokenKind::ShlEq,
            Base::Shr => TokenKind::Shr,
            Base::ShrEq => TokenKind::ShrEq,
            Base::Lt => TokenKind::Lt,
            Base::Le => TokenKind::Le,
            Base::Gt => TokenKind::Gt,
            Base::Ge => TokenKind::Ge,
            Base::EqEq => TokenKind::EqEq,
            Base::NotEq => TokenKind::NotEq,
            Base::Eq => TokenKind::Eq,
        }
    }
}
