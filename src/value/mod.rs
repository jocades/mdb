use std::sync::Arc;

pub mod codec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Type {
    Int = 1,
    Bool,
    Text,
}

impl Type {
    pub fn from_u8(tag: u8) -> Self {
        assert!(tag >= Type::Int as u8 && tag <= Type::Text as u8);
        unsafe { std::mem::transmute(tag) }
    }
}

#[derive(Debug, PartialEq, Eq, derive_more::From)]
pub enum Value {
    #[from]
    Int(i64),
    #[from]
    Bool(bool),
    #[from(&str, Arc<str>)]
    Text(Arc<str>),
}

impl Value {}
