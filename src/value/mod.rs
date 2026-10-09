pub mod codec;

use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Type {
    Int = 1,
    Bool,
    Text,
}

impl Type {
    pub fn tag(self) -> u8 {
        self as u8
    }

    pub fn from_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            1 => Type::Int,
            2 => Type::Bool,
            3 => Type::Text,
            _ => return None,
        })
    }

    pub fn from_tag_unchecked(tag: u8) -> Self {
        debug_assert!(tag >= Type::Int as u8 && tag <= Type::Text as u8);
        unsafe { std::mem::transmute(tag) }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, derive_more::From, derive_more::Display)]
#[display("{_0}")]
pub enum Value {
    #[from]
    Int(i64),
    #[from]
    Bool(bool),
    #[from(&str, Arc<str>)]
    Text(Arc<str>),
}

pub type Tuple = Vec<Value>;

impl Value {
    pub fn ty(&self) -> Type {
        match self {
            Value::Int(_) => Type::Int,
            Value::Bool(_) => Type::Bool,
            Value::Text(_) => Type::Text,
        }
    }
}

use crate::sql::ast::Lit;

impl From<&Lit> for Value {
    fn from(lit: &Lit) -> Self {
        match lit {
            Lit::Int(n) => Value::Int(*n),
            Lit::Bool(b) => Value::Bool(*b),
            Lit::String(s) => Value::Text(s.clone()),
        }
    }
}
