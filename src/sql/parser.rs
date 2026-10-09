#![allow(dead_code)]
use std::sync::Arc;

use super::ast::{self, BinOp, ColumnDef, Expr, Ident, Lit, Stmt};
use super::lexer::{self, Kind, Lexer, Span};
use crate::value::Type;

#[derive(Debug, derive_more::From)]
#[allow(dead_code)]
pub enum Error {
    #[from]
    Lex(lexer::Error),
    Expected {
        expected: Vec<Kind>,
        found: Kind,
        span: Span,
    },
    ExpectedExpression(Span),
    ExpectedType(Span),
    UnknwonType(Span),
    UnexpectedEof(Span),
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
    Parser {
        source,
        tokens,
        cursor: 0,
    }
    .parse()
}

pub fn parse_one(source: &str) -> Result<Stmt> {
    // todo: this is wasteful since there might be more
    // than one statement and we are consuming all tokens
    let tokens = lex(source)?;
    Parser {
        source,
        tokens,
        cursor: 0,
    }
    .parse_one()
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser<'_> {
    fn parse(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();

        while !self.is_eof() {
            let stmt = self.stmt()?;
            if !self.is_eof() {
                self.expect(Kind::Semi)?;
            }
            stmts.push(stmt);
        }

        Ok(stmts)
    }

    fn parse_one(&mut self) -> Result<Stmt> {
        let stmt = self.stmt()?;
        self.eat(Kind::Semi); // optional semicolon
        ensure!(self.is_eof(), Error::UnexpectedEof(self.peek().span));
        Ok(stmt)
    }

    fn stmt(&mut self) -> Result<Stmt> {
        let t = self.peek();
        match t.kind {
            Kind::Create => {
                self.bump();
                self.create()
            }
            Kind::Drop => {
                self.bump();
                self.drop()
            }
            Kind::Insert => {
                self.bump();
                self.insert()
            }
            Kind::Select => {
                self.bump();
                self.select()
            }
            _ => {
                bail!(Error::Expected {
                    expected: vec![Kind::Create, Kind::Drop, Kind::Insert, Kind::Select],
                    found: t.kind,
                    span: t.span,
                });
            }
        }
    }

    fn create(&mut self) -> Result<Stmt> {
        self.expect(Kind::Table)?;
        let name = self.ident()?;
        self.expect(Kind::LParen)?;
        let mut defs = vec![];
        loop {
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
            defs.push(ColumnDef { name, ty });
            if !self.eat(Kind::Comma) {
                break;
            }
        }
        self.expect(Kind::RParen)?;
        Ok(Stmt::CreateTable { name, defs })
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
        let vals = self.expr_list()?;
        self.expect(Kind::RParen)?;
        Ok(Stmt::Insert(ast::Insert { into, vals }))
    }

    fn select(&mut self) -> Result<Stmt> {
        let cols = self.expr_list()?;
        let t = self.peek();
        let from = match t.kind {
            Kind::From => {
                self.bump();
                Some(self.ident()?)
            }
            Kind::Semi => None,
            _ => bail!(Error::Expected {
                expected: vec![Kind::From, Kind::Semi],
                found: t.kind,
                span: t.span,
            }),
        };

        let were = self.eat(Kind::Where).then(|| self.expr()).transpose()?;

        Ok(Stmt::Select(ast::Select { cols, from, were }))
    }

    #[rustfmt::skip]
    fn expr_list(&mut self) -> Result<Vec<Expr>> {
        let mut exprs = Vec::new();
        loop {
            exprs.push(self.expr()?);
            if !self.eat(Kind::Comma) { break; }
        }
        Ok(exprs)
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
            Kind::Star => Expr::Wildcard,
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

    fn bump(&mut self) {
        if !self.is_eof() {
            self.cursor += 1;
        }
    }

    fn eat(&mut self, kind: Kind) -> bool {
        if self.peek().kind != kind {
            return false;
        }
        self.bump();
        true
    }

    // fn prev(&self) -> Token {
    //     self.tokens[self.cursor - 1]
    // }

    fn is_eof(&self) -> bool {
        self.peek().kind == Kind::Eof
    }

    // fn matches(&mut self, kind: Kind) -> bool {}

    fn expect(&mut self, kind: Kind) -> Result<Token> {
        let t = self.peek();
        if t.kind != kind {
            bail!(match t.kind {
                Kind::Eof => Error::UnexpectedEof(t.span),
                _ => Error::Expected {
                    expected: vec![kind],
                    found: t.kind,
                    span: t.span,
                },
            });
        };
        self.bump();
        Ok(t)
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
