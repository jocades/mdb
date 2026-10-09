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

    #[allow(unused)]
    pub fn from_tag_unchecked(tag: u8) -> Self {
        debug_assert!(tag >= Type::Int as u8 && tag <= Type::Text as u8);
        unsafe { std::mem::transmute(tag) }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, derive_more::Display)]
#[display("{_0}")]
pub enum Value {
    Int(i64),
    Bool(bool),
    String(Arc<str>),
}

pub type Tuple = Vec<Value>;

impl Value {
    pub fn ty(&self) -> Type {
        match self {
            Value::Int(_) => Type::Int,
            Value::Bool(_) => Type::Bool,
            Value::String(_) => Type::Text,
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Int(a), Value::Int(b)) => a.partial_cmp(b),
            (Self::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            (Self::String(a), Value::String(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}
