use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use crate::bytes::{BufRead, BufWrite};
use crate::heap::{Heap, Rid};
use crate::storage::{Disk, PageId, Pool};
use crate::value::{Type, Value};

const CATALOG_ROOT: PageId = 1;

/** Stores table definitions and metadata */
#[derive(Debug)]
pub struct Catalog {
    pub heap: Heap,
    pub tables: HashMap<Arc<str>, Table>,
}

impl Catalog {
    pub fn open(pool: &mut Pool<impl Disk>) -> io::Result<Self> {
        if pool.len() == 1 {
            // only the header exists, create the catalog eagerly
            let heap = Heap::create(pool)?;
            assert_eq!(heap.first(), CATALOG_ROOT);
            pool.flush()?;
            return Ok(Self {
                heap,
                tables: HashMap::new(),
            });
        }

        let heap = Heap::open(pool, CATALOG_ROOT)?;
        let mut tables = HashMap::new();

        let mut scan = heap.scan();
        while let Some((rid, record)) = scan.next(pool)? {
            let (name, first_page, columns) = decode(record);
            let table = Table {
                heap: Heap::open(pool, first_page)?,
                schema: Schema::new(columns),
                catalog_rid: rid,
            };
            tables.insert(name.into(), table);
        }

        Ok(Self { heap, tables })
    }

    // fn create_table(&mut self, pool: &mut Pool<impl Disk>, name ) -> io::Result<()> {
    //     let heap = Heap::create(pool).unwrap();
    //     Ok(())
    // }
}

/// Encode a catalog reocrd
pub fn encode(name: &str, first_page: PageId, columns: &[Column]) -> Vec<u8> {
    let mut buf = Vec::new();
    assert!(name.len() <= u16::MAX as usize);
    assert!(columns.len() <= u16::MAX as usize);
    buf.write_u16(name.len() as u16);
    buf.write_slice(name.as_bytes());
    buf.write_u32(first_page);
    buf.write_u16(columns.len() as u16);
    for col in columns {
        buf.write_u16(col.name.len() as u16);
        buf.write_slice(col.name.as_bytes());
        buf.write_u8(col.ty as u8);
    }
    buf
}

/// Decode a catalog record
pub fn decode(mut buf: &[u8]) -> (String, PageId, Vec<Column>) {
    let name_len = buf.read_u16() as usize;
    let name = buf.read_str(name_len).to_owned();
    let first_page = buf.read_u32();
    let cols_len = buf.read_u16() as usize;
    let mut columns = Vec::with_capacity(cols_len);
    for _ in 0..cols_len {
        let name_len = buf.read_u16() as usize;
        let name = buf.read_str(name_len).to_owned();
        let ty = Type::from_u8(buf.read_u8());
        columns.push(Column::new(name, ty))
    }
    (name, first_page, columns)
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

#[derive(Debug)]
pub struct Schema {
    pub columns: Vec<Column>,
    pub by_name: HashMap<Arc<str>, usize>,
}

impl Schema {
    pub fn new(columns: Vec<Column>) -> Self {
        let by_name = columns
            .iter()
            .enumerate()
            .map(|(idx, col)| (col.name.clone(), idx))
            .collect();

        Self { columns, by_name }
    }

    fn index_of(&self, col: &str) -> Option<usize> {
        self.by_name.get(col).copied()
    }
}

#[derive(Debug)]
pub struct Table {
    pub heap: Heap,
    pub schema: Schema,
    // /// Where this table's description lives in the catalog heap
    pub catalog_rid: Rid,
}

impl Table {
    pub fn insert(&mut self, pool: &mut Pool<impl Disk>, row: &[Value]) -> Rid {
        // let bytes = encode(&self.schema, row);
        // self.heap.insert(pool, &bytes).unwrap()
        todo!()
    }

    // pub fn get(&self, pool: &mut Pool<impl Disk>, rid: Rid) -> Option<Vec<Value>> {
    //     self.heap
    //         .get(pool, rid)
    //         .unwrap()
    //         .map(|bytes| decode(&self.schema, bytes))
    // }
}
