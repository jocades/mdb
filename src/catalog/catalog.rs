use std::{collections::HashMap, io, sync::Arc};

use crate::bytes::{BufRead, BufWrite};
use crate::heap::Heap;
use crate::storage::{Disk, PageId, Pool};
use crate::value::Type;

use super::{Column, Error, Schema, Table};

const CATALOG_ROOT: PageId = 1;

/// Stores table definitions and metadata
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
                name: name.clone(),
                heap: Heap::open(pool, first_page)?,
                schema: Arc::new(Schema::new_unchecked(columns)),
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
        let catalog_record = encode(&name, heap.first(), &schema.columns);
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
pub fn encode(name: &str, first_page: PageId, cols: &[Column]) -> Vec<u8> {
    assert!(name.len() <= u16::MAX as usize);
    assert!(cols.len() <= u16::MAX as usize);
    let mut buf = Vec::new();
    buf.write_u16(name.len() as u16);
    buf.write_slice(name.as_bytes());
    buf.write_u32(first_page);
    buf.write_u16(cols.len() as u16);
    for col in cols {
        buf.write_u16(col.name.len() as u16);
        buf.write_slice(col.name.as_bytes());
        buf.write_u8(col.ty.tag());
    }
    buf
}

/// Decode a catalog record
pub fn decode(mut buf: &[u8]) -> (Arc<str>, PageId, Vec<Column>) {
    let name_len = buf.read_u16() as usize;
    let name: Arc<str> = buf.read_str(name_len).into();
    let first_page = buf.read_u32();
    let cols_len = buf.read_u16() as usize;
    let mut cols = Vec::with_capacity(cols_len);
    for _ in 0..cols_len {
        let name_len = buf.read_u16() as usize;
        let name: Arc<str> = buf.read_str(name_len).into();
        let ty = Type::from_tag(buf.read_u8()).unwrap();
        cols.push(Column::new(name, ty))
    }
    (name, first_page, cols)
}
