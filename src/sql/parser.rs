#![allow(dead_code)]
use std::sync::Arc;

use super::ast::{BinOp, Column, Create, Expr, Lit, Stmt, Type};
use super::lexer::{self, Kind, Lexer, Span};

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

pub type Result<T> = std::result::Result<T, Error>;

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

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser<'_> {
    fn parse(mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();

        while !self.is_eof() {
            stmts.push(self.stmt()?);
        }

        Ok(stmts)
    }

    fn stmt(&mut self) -> Result<Stmt> {
        let t = self.peek();
        let stmt = match t.kind {
            Kind::Create => {
                self.bump();
                self.expect(Kind::Table)?;
                let name = self.ident()?;

                self.expect(Kind::LParen)?;
                let mut columns = vec![];
                loop {
                    let name = self.ident()?;
                    let t = self.peek();
                    ensure!(t.kind == Kind::Ident, Error::ExpectedType(t.span));
                    let lexeme = t.lexeme(self.source);
                    let ty = match_case_insensitive!(lexeme,
                        "int" => Type::Integer,
                        "bool" => Type::Boolean,
                        "string" => Type::String,
                        _ => bail!(Error::UnknwonType(t.span)),
                    );
                    self.bump();
                    columns.push(Column { name, ty });
                    match self.peek().kind {
                        Kind::Comma => self.bump(),
                        _ => break,
                    }
                }
                self.expect(Kind::RParen)?;
                Stmt::Create(Create::Table { name, columns })
            }

            Kind::Select => {
                self.bump();
                let mut projection = Vec::new();

                loop {
                    projection.push(self.expr()?);
                    if self.matches(Kind::Comma) {
                        break;
                    }
                }

                let t = self.peek();
                let relation = match t.kind {
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

                Stmt::Select {
                    projection,
                    relation,
                }
            }

            Kind::Insert => {
                todo!()
            }
            _ => {
                bail!(Error::Expected {
                    expected: vec![Kind::Create, Kind::Select],
                    found: t.kind,
                    span: t.span,
                });
            }
        };

        self.expect(Kind::Semi)?;
        Ok(stmt)
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
            Kind::String => {
                let s = &self.source[t.span.start + 1..t.span.end - 1];
                Expr::Lit(Lit::String(Arc::from(s)))
            }
            Kind::Ident => {
                let lexeme = t.lexeme(&self.source);
                Expr::Ident(Arc::from(lexeme))
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

    fn bump(&mut self) {
        if !self.is_eof() {
            self.cursor += 1;
        }
    }

    fn eat(&mut self) -> Token {
        let t = self.peek();
        self.bump();
        t
    }

    // fn prev(&self) -> Token {
    //     self.tokens[self.cursor - 1]
    // }

    fn is_eof(&self) -> bool {
        self.peek().kind == Kind::Eof
    }

    fn matches(&mut self, kind: Kind) -> bool {
        if self.peek().kind != kind {
            return false;
        }
        self.bump();
        true
    }

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
        Ok(self.eat())
    }

    fn ident(&mut self) -> Result<Arc<str>> {
        let t = self.expect(Kind::Ident)?;
        let lexeme = t.lexeme(&self.source);
        Ok(Arc::from(lexeme))
    }
}
