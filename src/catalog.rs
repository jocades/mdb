use std::{collections::HashMap, io, sync::Arc};

use crate::bytes::{BufRead, BufWrite};
use crate::heap::{self, Heap, Rid};
use crate::storage::{Disk, PageId, Pool};
use crate::value::{Type, Value, codec};

const CATALOG_ROOT: PageId = 1;

/** Stores table definitions and metadata */
#[derive(Debug)]
pub struct Catalog {
    pub heap: Heap,
    pub tables: HashMap<Arc<str>, Table>,
}

#[derive(Debug, derive_more::From)]
pub enum Error {
    #[from]
    Io(io::Error),
    #[from]
    Heap(heap::Error),
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
                name: name.clone(),
                heap: Heap::open(pool, first_page)?,
                schema: Arc::new(Schema::new(columns)),
                catalog_rid: rid,
            };
            tables.insert(name, table);
        }

        Ok(Self { heap, tables })
    }

    pub fn create_table(
        &mut self,
        pool: &mut Pool<impl Disk>,
        name: Arc<str>,
        schema: Schema,
    ) -> Result<(), Error> {
        debug_assert!(!self.tables.contains_key(&name));

        let heap = Heap::create(pool)?;
        let first_page = pool.alloc()?;
        let catalog_record = encode(&name, first_page, &schema.columns);
        let catalog_rid = self.heap.insert(pool, &catalog_record)?;

        // let catalog_rid = match self.heap.insert(pool, &catalog_record) {
        //     Ok(rid) => rid,
        //     Err(e) => {
        //         _ = heap.destroy(pool);
        //         bail!(e)
        //     }
        // };

        let table = Table {
            name: name.clone(),
            heap,
            schema: Arc::new(schema),
            catalog_rid,
        };

        self.tables.insert(name, table);
        Ok(())
    }

    // caller must gurantee table exists
    pub fn drop_table(&mut self, pool: &mut Pool<impl Disk>, name: &str) -> Result<(), Error> {
        // delete the catalog record before freeing the pages.
        // if we crash in between, we leak pages, the reverse order would
        // leave the catalog pointing to freed pages, which is corruption,
        // a leak is the safer failure
        let table = self.tables.remove(name).expect("caller checked existence");

        if let Err(e) = self.heap.delete(pool, table.catalog_rid) {
            // on failure put the table back; this way memory always
            // matches what a restart would produce
            self.tables.insert(name.into(), table);
            bail!(e)
        }

        table.heap.destroy(pool)?; // if this fails we leak pages but memory matches disk
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Table> {
        self.tables.get_mut(name)
    }
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
pub fn decode(mut buf: &[u8]) -> (Arc<str>, PageId, Vec<Column>) {
    let name_len = buf.read_u16() as usize;
    let name: Arc<str> = buf.read_str(name_len).into();
    let first_page = buf.read_u32();
    let cols_len = buf.read_u16() as usize;
    let mut columns = Vec::with_capacity(cols_len);
    for _ in 0..cols_len {
        let name_len = buf.read_u16() as usize;
        let name: Arc<str> = buf.read_str(name_len).into();
        let ty = Type::from_tag(buf.read_u8()).unwrap();
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
    /// Caller must gurantee there are no repeated columns
    pub fn new(columns: Vec<Column>) -> Self {
        let by_name = columns
            .iter()
            .enumerate()
            .map(|(idx, col)| (col.name.clone(), idx))
            .collect();

        Self { columns, by_name }
    }

    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            by_name: HashMap::new(),
        }
    }

    pub fn index_of(&self, col: &str) -> Option<usize> {
        self.by_name.get(col).copied()
    }
}

#[derive(Debug)]
pub struct Table {
    pub name: Arc<str>,
    pub heap: Heap,
    pub schema: Arc<Schema>,
    // /// Where this table's description lives in the catalog heap
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
