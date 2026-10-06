use std::{io, path::Path};

use crate::catalog::{self, Catalog, Column, Schema, Table};
use crate::heap::Heap;
use crate::sql;
use crate::sql::ast::Ident;
use crate::storage::{DEFAULT_POOL_CAPACITY, Disk, FileDisk, MemDisk, Pool};

pub struct Database<D: Disk> {
    pub pool: Pool<D>,
    pub catalog: Catalog,
}

impl<D: Disk> Database<D> {
    pub fn with_disk(disk: D, pool_cap: usize) -> io::Result<Self> {
        let mut pool = Pool::with_capacity(disk, pool_cap)?;
        let catalog = Catalog::open(&mut pool)?;
        Ok(Self { pool, catalog })
    }
}

impl Database<FileDisk> {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::with_disk(FileDisk::open(path)?, DEFAULT_POOL_CAPACITY)
    }
}

impl Database<MemDisk> {
    pub fn memory() -> Self {
        // `MemDisk` is infallible
        Self::with_disk(MemDisk::default(), DEFAULT_POOL_CAPACITY).unwrap()
    }
}

#[derive(Debug, derive_more::From)]
pub enum Error {
    #[from]
    Io(io::Error),
    #[from]
    Parse(sql::parser::Error),
    TableExists(Ident),
}

pub type Result<T> = std::result::Result<T, Error>;

impl<D: Disk> Database<D> {
    pub fn execute(&mut self, sql: &str) -> Result<()> {
        use sql::ast::Stmt;

        let stmts = sql::parse(sql)?;
        for stmt in stmts {
            match stmt {
                Stmt::CreateTable { name, columns } => {
                    ensure!(
                        !self.catalog.tables.contains_key(&name.lexeme),
                        Error::TableExists(name)
                    );

                    let heap = Heap::create(&mut self.pool).unwrap();
                    let schema = Schema::new(
                        columns
                            .into_iter()
                            .map(|col| Column::new(col.name.lexeme, col.ty))
                            .collect(),
                    );

                    let catalog_record =
                        catalog::encode(&name.lexeme, heap.first(), &schema.columns);

                    let catalog_rid = self
                        .catalog
                        .heap
                        .insert(&mut self.pool, &catalog_record)
                        .unwrap();

                    let table = Table {
                        schema,
                        heap,
                        catalog_rid,
                    };

                    self.catalog.tables.insert(name.lexeme, table);
                }
                Stmt::Select {
                    projection,
                    relation,
                } => todo!(),
            }
        }
        Ok(())
    }
}
