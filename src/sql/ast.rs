use std::sync::Arc;

use crate::value::Type;

use super::lexer::Span;

#[derive(Debug)]
pub enum Stmt {
    CreateTable {
        name: Ident,
        columns: Vec<ColumnDef>,
    },
    Select {
        projection: Vec<Expr>,
        relation: Option<Ident>,
    },
}

#[derive(Debug)]
pub struct ColumnDef {
    pub name: Ident,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub lexeme: Arc<str>,
    pub span: Span,
}

#[derive(Debug)]
pub enum Expr {
    Lit(Lit),
    Ident(Arc<str>),
    Bin(Box<Expr>, BinOp, Box<Expr>),
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
    Eq, Ne,
    Gt, Ge,
    Lt, Le,
    Add, Sub,
    Mul, Div,
}
