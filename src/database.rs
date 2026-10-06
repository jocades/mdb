use std::{io, path::Path};

use crate::catalog::{self, Catalog, Column, Schema, Table};
use crate::heap::Heap;
use crate::sql;
use crate::sql::ast::{Expr, Ident, Lit};
use crate::storage::{DEFAULT_POOL_CAPACITY, Disk, FileDisk, MemDisk, Pool};
use crate::value::{Type, Value, codec};

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
    TableNotFound(Ident),
    ColumnNotFound {
        column: Ident,
        table: Ident,
    },
    ArityMismatch {
        table: Ident,
        expected: u16,
        found: u16,
    },
    TypeMismatch {
        expected: Type,
        found: Type,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

impl<D: Disk> Database<D> {
    pub fn execute(&mut self, sql: &str) -> Result<()> {
        use sql::ast::Stmt;

        let stmts = sql::parse(sql)?;
        for stmt in stmts {
            println!("{stmt:?}");
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
                Stmt::Insert {
                    into,
                    values: exprs,
                } => {
                    let Some(table) = self.catalog.tables.get_mut(&into.lexeme) else {
                        bail!(Error::TableNotFound(into))
                    };

                    // arity check
                    ensure!(
                        exprs.len() == table.schema.columns.len(),
                        Error::ArityMismatch {
                            table: into,
                            expected: table.schema.columns.len() as u16,
                            found: exprs.len() as u16,
                        }
                    );

                    let mut tuple = Vec::with_capacity(exprs.len());
                    for (expr, col) in exprs.into_iter().zip(&table.schema.columns) {
                        let value = eval(&expr);
                        let ty = value.ty();
                        ensure!(
                            ty == col.ty,
                            Error::TypeMismatch {
                                expected: col.ty,
                                found: ty,
                            }
                        );
                        tuple.push(value);
                    }

                    let record = codec::encode(&table.schema, &tuple);
                    table.heap.insert(&mut self.pool, &record).unwrap();
                }
                Stmt::Select {
                    projection,
                    relation,
                } => match relation {
                    None => {}
                    Some(name) => {
                        let Some(table) = self.catalog.tables.get(&name.lexeme) else {
                            bail!(Error::TableNotFound(name));
                        };

                        for expr in &projection {
                            match expr {
                                Expr::Ident(col) => {
                                    ensure!(
                                        table.schema.by_name.contains_key(&name.lexeme),
                                        Error::ColumnNotFound {
                                            column: col.clone(),
                                            table: name.clone(),
                                        }
                                    )
                                }
                                Expr::Wildcard => {}
                                _ => todo!("{expr:?} not implemented in select"),
                            }
                        }

                        let mut result = Vec::new();

                        let mut scan = table.heap.scan();
                        while let Some((rid, record)) = scan.next(&mut self.pool).unwrap() {
                            let tuple = codec::decode(&table.schema, record);
                            result.push((rid, tuple));
                        }

                        dbg!(result);
                    }
                },
            }
        }
        Ok(())
    }
}

struct Context<'a, D: Disk> {
    pool: &'a mut Pool<D>,
    catalog: &'a Catalog,
}

fn eval(expr: &Expr) -> Value {
    match expr {
        Expr::Lit(lit) => match lit {
            Lit::Int(n) => Value::Int(*n),
            Lit::Bool(b) => Value::Bool(*b),
            Lit::String(s) => Value::Text(s.clone()),
        },
        Expr::Ident(_) => todo!(),
        Expr::Bin(expr, bin_op, expr1) => todo!(),
        Expr::Wildcard => todo!(),
    }
}
