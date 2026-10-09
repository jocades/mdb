use std::sync::Arc;

use super::ast::{self, BinOp, ColumnDef, Expr, Ident, Lit, SelectItem, Stmt};
use super::lexer::{self, Kind, Lexer, Span};
use crate::value::Type;

#[derive(Debug, derive_more::From)]
#[allow(dead_code)]
pub enum Error {
    #[from]
    Lex(lexer::Error),
    Expected {
        expected: Expected,
        found: Kind,
        span: Span,
    },
    ExpectedExpression(Span),
    ExpectedType(Span),
    UnknwonType(Span),
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    span: Span,
}

impl Token {
    fn lexeme<'a>(&self, source: &'a str) -> &'a str {
        &source[self.span.start..self.span.end]
    }
}

fn lex(src: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();

    for (kind, span) in Lexer::new(src).spanned() {
        tokens.push(Token { kind: kind?, span });
    }

    let eof = src.len();
    tokens.push(Token {
        kind: Kind::Eof,
        span: Span::from(eof..eof),
    });

    Ok(tokens)
}

pub fn parse(source: &str) -> Result<Vec<Stmt>> {
    let tokens = lex(source)?;
    Parser::new(source, tokens).parse()
}

pub fn parse_one(source: &str) -> Result<Stmt> {
    // todo: this is wasteful since there might be more
    // than one statement and we are consuming all tokens
    let tokens = lex(source)?;
    Parser::new(source, tokens).parse_one()
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Expected(u64);

impl Expected {
    pub fn insert(&mut self, k: Kind) {
        self.0 |= 1u64 << (k as u8);
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn iter(self) -> impl Iterator<Item = Kind> {
        let mut bits = self.0;
        std::iter::from_fn(move || {
            if bits == 0 {
                return None;
            }
            let i = bits.trailing_zeros() as u8;
            bits &= bits - 1;
            Some(Kind::from_u8(i))
        })
    }
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
    expected: Expected,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            cursor: 0,
            expected: Expected::default(),
        }
    }

    #[rustfmt::skip]
    fn parse(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();

        loop {
            while self.eat(Kind::Semi) {}
            if self.is_eof() { break; }
            let stmt = self.stmt()?;
            self.finish_stmt()?;
            stmts.push(stmt);
        }

        Ok(stmts)
    }

    fn parse_one(&mut self) -> Result<Stmt> {
        let stmt = self.stmt()?;
        self.finish_stmt()?;
        self.expect(Kind::Eof)?;
        Ok(stmt)
    }

    #[rustfmt::skip]
    fn stmt(&mut self) -> Result<Stmt> {
        if self.eat(Kind::Create) { return self.create(); }
        if self.eat(Kind::Insert) { return self.insert(); }
        if self.eat(Kind::Select) { return self.select(); }
        if self.eat(Kind::Drop)   { return self.drop(); }
        Err(self.expect_error())
    }

    fn finish_stmt(&mut self) -> Result<()> {
        ensure!(self.eat(Kind::Semi) || self.is_eof(), self.expect_error());
        Ok(())
    }

    fn create(&mut self) -> Result<Stmt> {
        self.expect(Kind::Table)?;
        let name = self.ident()?;
        self.expect(Kind::LParen)?;
        let defs = self.comma_sep(Self::column_def)?;
        self.expect(Kind::RParen)?;
        Ok(Stmt::CreateTable { name, defs })
    }

    fn column_def(&mut self) -> Result<ColumnDef> {
        let name = self.ident()?;
        let t = self.peek();
        ensure!(t.kind == Kind::Ident, Error::ExpectedType(t.span));
        let lexeme = t.lexeme(self.source);
        let ty = match_case_insensitive!(lexeme,
            "int" => Type::Int,
            "bool" => Type::Bool,
            "text" => Type::Text,
            _ => bail!(Error::UnknwonType(t.span)),
        );
        self.bump();
        Ok(ColumnDef { name, ty })
    }

    fn drop(&mut self) -> Result<Stmt> {
        self.expect(Kind::Table)?;
        let name = self.ident()?;
        Ok(Stmt::DropTable { name })
    }

    fn insert(&mut self) -> Result<Stmt> {
        self.expect(Kind::Into)?;
        let into = self.ident()?;
        self.expect(Kind::Values)?;
        self.expect(Kind::LParen)?;
        let vals = self.comma_sep(Self::expr)?;
        self.expect(Kind::RParen)?;
        Ok(Stmt::Insert(ast::Insert { into, vals }))
    }

    fn select(&mut self) -> Result<Stmt> {
        let cols = self.comma_sep(Self::select_item)?;
        let from = self.eat(Kind::From).then(|| self.ident()).transpose()?;
        let were = self.eat(Kind::Where).then(|| self.expr()).transpose()?;
        Ok(Stmt::Select(ast::Select { cols, from, were }))
    }

    fn comma_sep<R>(&mut self, f: fn(&mut Self) -> Result<R>) -> Result<Vec<R>> {
        let mut out = Vec::new();
        loop {
            out.push(f(self)?);
            if !self.eat(Kind::Comma) {
                break;
            }
        }
        Ok(out)
    }

    fn select_item(&mut self) -> Result<SelectItem> {
        if self.eat(Kind::Star) {
            return Ok(SelectItem::Wildcard);
        }
        Ok(SelectItem::Expr {
            expr: self.expr()?,
            alias: None,
        })
    }

    fn expr(&mut self) -> Result<Expr> {
        self.eq()
    }

    fn bin(&mut self, sub: fn(&mut Self) -> Result<Expr>, ops: &[Kind]) -> Result<Expr> {
        let mut expr = sub(self)?;
        loop {
            let t = self.peek();
            if !ops.contains(&t.kind) {
                break;
            }
            self.bump();
            let rhs = sub(self)?;
            let op = match t.kind {
                Kind::Eq => BinOp::Eq,
                Kind::BangEq => BinOp::Ne,
                Kind::Gt => BinOp::Gt,
                Kind::GtEq => BinOp::Ge,
                Kind::Lt => BinOp::Lt,
                Kind::LtEq => BinOp::Le,
                Kind::Plus => BinOp::Add,
                Kind::Minus => BinOp::Sub,
                Kind::Star => BinOp::Mul,
                Kind::Slash => BinOp::Div,
                _ => unreachable!(),
            };
            expr = Expr::Bin(Box::new(expr), op, Box::new(rhs));
        }
        Ok(expr)
    }

    fn eq(&mut self) -> Result<Expr> {
        self.bin(Self::cmp, &[Kind::Eq, Kind::BangEq])
    }

    fn cmp(&mut self) -> Result<Expr> {
        self.bin(Self::term, &[Kind::Gt, Kind::GtEq, Kind::Lt, Kind::LtEq])
    }

    fn term(&mut self) -> Result<Expr> {
        self.bin(Self::factor, &[Kind::Plus, Kind::Minus])
    }

    fn factor(&mut self) -> Result<Expr> {
        self.bin(Self::primary, &[Kind::Star, Kind::Slash])
    }

    fn primary(&mut self) -> Result<Expr> {
        let t = self.peek();

        let expr = match t.kind {
            Kind::Number => {
                let n = t.lexeme(&self.source).parse().unwrap();
                Expr::Lit(Lit::Int(n))
            }
            Kind::True => Expr::Lit(Lit::Bool(true)),
            Kind::False => Expr::Lit(Lit::Bool(false)),
            Kind::String => {
                let s = &self.source[t.span.start + 1..t.span.end - 1];
                Expr::Lit(Lit::String(Arc::from(s)))
            }
            Kind::Ident => {
                let lexeme = t.lexeme(&self.source);
                Expr::Ident(Ident {
                    lexeme: Arc::from(lexeme),
                    span: t.span,
                })
            }
            Kind::LParen => {
                self.bump();
                let expr = self.expr()?;
                self.expect(Kind::RParen)?;
                return Ok(expr);
            }
            _ => {
                bail!(Error::ExpectedExpression(t.span));
            }
        };

        self.bump();
        Ok(expr)
    }

    fn peek(&self) -> Token {
        self.tokens[self.cursor]
    }

    fn is_eof(&mut self) -> bool {
        self.check(Kind::Eof)
    }

    #[rustfmt::skip]
    fn check(&mut self, kind: Kind) -> bool {
        let hit = self.peek().kind == kind;
        if !hit { self.expected.insert(kind); }
        hit
    }

    #[rustfmt::skip]
    fn bump(&mut self) -> Token {
        self.expected.clear();
        let t = self.peek();
        if t.kind != Kind::Eof { self.cursor += 1; }
        t
    }

    #[rustfmt::skip]
    fn eat(&mut self, kind: Kind) -> bool {
        let hit = self.check(kind);
        if hit { self.bump(); }
        hit
    }

    fn expect(&mut self, kind: Kind) -> Result<Token> {
        ensure!(self.check(kind), self.expect_error());
        Ok(self.bump())
    }

    fn expect_error(&self) -> Error {
        let t = self.peek();
        Error::Expected {
            expected: self.expected,
            found: t.kind,
            span: t.span,
        }
    }

    fn ident(&mut self) -> Result<Ident> {
        let t = self.expect(Kind::Ident)?;
        let lexeme = t.lexeme(&self.source);
        Ok(Ident {
            lexeme: Arc::from(lexeme),
            span: t.span,
        })
    }
}
