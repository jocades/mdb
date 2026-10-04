#![allow(unused)]
use std::sync::Arc;

#[derive(Debug)]
pub enum Stmt {
    Create(Create),
    Select {
        projection: Vec<Expr>,
        relation: Option<Arc<str>>,
    },
}

#[derive(Debug)]
pub enum Create {
    Table {
        name: Arc<str>,
        columns: Vec<Column>,
    },
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

#[derive(Debug)]
pub enum Type {
    Integer,
    Boolean,
    String,
}

#[derive(Debug)]
pub struct Column {
    pub name: Arc<str>,
    pub ty: Type,
}
