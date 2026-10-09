use std::sync::Arc;

use crate::value::Type;

use super::lexer::Span;

#[derive(Debug)]
pub enum Stmt {
    CreateTable { name: Ident, defs: Vec<ColumnDef> },
    DropTable { name: Ident },
    Insert(Insert),
    Select(Select),
}

#[derive(Debug)]
pub struct Insert {
    pub into: Ident,
    pub vals: Vec<Expr>,
}

#[derive(Debug)]
pub struct Select {
    pub cols: Vec<Expr>,
    pub from: Option<Ident>,
    pub were: Option<Expr>,
}

#[derive(Debug)]
pub struct ColumnDef {
    pub name: Ident,
    pub ty: Type,
}

#[derive(Debug, Clone, derive_more::Display)]
#[display("{lexeme}")]
pub struct Ident {
    pub lexeme: Arc<str>,
    pub span: Span,
}

#[derive(Debug)]
pub enum Expr {
    Lit(Lit),
    Ident(Ident),
    Bin(Box<Expr>, BinOp, Box<Expr>),
    Wildcard, // *
}

#[derive(Debug)]
pub enum Lit {
    Int(i64),
    Bool(bool),
    String(Arc<str>),
}

#[derive(Debug, Clone, Copy)]
#[rustfmt::skip]
pub enum BinOp {
    Add, Sub,
    Mul, Div,
    Eq, Ne,
    Gt, Ge,
    Lt, Le,
}
