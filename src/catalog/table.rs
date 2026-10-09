use std::sync::Arc;

use crate::heap::{Heap, Rid};
use crate::storage::{Disk, Pool};
use crate::value::{Type, Value, codec};

use super::Error;

/// A named schema backed by a heap. This is what the catalog hands out.
#[derive(Debug)]
pub struct Table {
    pub name: Arc<str>,
    pub schema: Arc<Schema>,
    pub heap: Heap,
    pub catalog_rid: Rid,
}

impl Table {
    pub fn insert(&mut self, pool: &mut Pool<impl Disk>, tuple: &[Value]) -> Result<Rid, Error> {
        let record = codec::encode(&self.schema, tuple);
        Ok(self.heap.insert(pool, &record)?)
    }

    pub fn get(&self, pool: &mut Pool<impl Disk>, rid: Rid) -> Result<Option<Vec<Value>>, Error> {
        Ok(self
            .heap
            .get(pool, rid)?
            .map(|record| codec::decode(&self.schema, record)))
    }
}

#[derive(Debug)]
pub struct Schema {
    pub columns: Vec<Column>,
}

use std::sync::LazyLock;
pub static EMPTY_SCHEMA: LazyLock<Arc<Schema>> = LazyLock::new(|| Arc::new(Schema::empty()));

impl Schema {
    /// Caller must guarantee there are no repeated columns
    pub fn new_unchecked(columns: Vec<Column>) -> Self {
        Self { columns }
    }

    pub const fn empty() -> Self {
        Self { columns: vec![] }
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|col| &*col.name == name)
    }

    pub fn add_column(&mut self, name: impl Into<Arc<str>>, ty: Type) {
        self.columns.push(Column::new(name, ty));
    }
}

#[derive(Debug)]
pub struct Column {
    pub name: Arc<str>,
    pub ty: Type,
}

impl Column {
    pub fn new(name: impl Into<Arc<str>>, ty: Type) -> Self {
        Self {
            name: name.into(),
            ty,
        }
    }
}
