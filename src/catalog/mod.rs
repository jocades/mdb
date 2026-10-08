mod catalog;
mod table;

pub use catalog::Catalog;
pub use table::{Column, EMPTY_SCHEMA, Schema, Table};

#[derive(Debug, derive_more::From)]
pub enum Error {
    #[from]
    Io(std::io::Error),
    #[from]
    Heap(crate::heap::Error),
}
