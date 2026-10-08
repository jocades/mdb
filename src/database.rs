use std::sync::Arc;
use std::{io, path::Path};

use crate::catalog::{self, Catalog, Column, Schema, Table};
use crate::exec::Tuple;
use crate::exec::{self, Context};
use crate::heap::Heap;
use crate::plan::{Plan, binder};
use crate::sql;
use crate::sql::ast::{Expr, Ident, Lit};
use crate::storage::{DEFAULT_POOL_CAPACITY, Disk, FileDisk, MemDisk, Pool};
use crate::value::{Type, Value, codec};

pub struct Database<D: Disk> {
    pub pool: Pool<D>,
    pub cata: Catalog,
}

impl<D: Disk> Database<D> {
    pub fn with_disk(disk: D, pool_cap: usize) -> io::Result<Self> {
        let mut pool = Pool::with_capacity(disk, pool_cap)?;
        let cata = Catalog::open(&mut pool)?;
        Ok(Self { pool, cata })
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
    #[from]
    Bind(binder::Error),
    #[from]
    Catalog(catalog::Error),
    #[from]
    Exec(exec::Error),
    TableExists(Ident),
    TableNotFound(Ident),
    ColumnNotFound {
        column: Ident,
        table: Ident,
    },
    ColumnTypeMismatch {
        column: Arc<str>,
        expected: Type,
        found: Type,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum QueryResult {
    Affected(usize),
    Rows {
        rows: Vec<Tuple>,
        schema: Arc<Schema>,
    },
}

impl<D: Disk> Database<D> {
    pub fn execute(&mut self, sql: &str) -> Result<()> {
        use sql::ast::Stmt;

        let stmts = sql::parse(sql)?;

        for stmt in stmts {
            println!("{stmt:?}");
            match stmt {
                Stmt::CreateTable { name, defs } => {
                    let (name, schema) = binder::bind_create_table(&self.cata, &name, &defs)?;
                    self.cata.create_table(&mut self.pool, name, schema)?;
                }
                Stmt::DropTable { name } => {
                    ensure!(
                        self.cata.get(&name.lexeme).is_some(),
                        binder::Error::TableNotFound(name)
                    );
                    self.cata.drop_table(&mut self.pool, &name.lexeme)?;
                }
                Stmt::Insert(insert) => {
                    let plan = binder::bind_insert(&self.cata, &insert)?;
                    let result = self.run(&plan)?;
                    dbg!(result);
                }
                Stmt::Select(select) => {
                    let plan = binder::bind_select(&self.cata, &select)?;
                    let result = self.run(&plan)?;
                    dbg!(&result);
                    match result {
                        QueryResult::Rows { rows, schema } => {
                            if rows.len() != 0 {
                                let mut b = tabled::builder::Builder::new();
                                b.push_record(schema.columns.iter().map(|col| col.name.as_ref()));
                                for row in &rows {
                                    b.push_record(row.iter().map(ToString::to_string));
                                }
                                println!("{}", b.build());
                            }
                            println!("({} rows)", rows.len());
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
        Ok(())
    }

    fn run(&mut self, plan: &Plan) -> Result<QueryResult> {
        let mut root = exec::build::<D>(plan, &self.cata);
        let mut cx = Context {
            pool: &mut self.pool,
            cata: &mut self.cata,
        };

        let mut rows = Vec::new();
        while let Some(tuple) = root.next(&mut cx)? {
            rows.push(tuple)
        }

        let result = match plan {
            Plan::Insert { .. } => match rows[0][0] {
                Value::Int(n) => QueryResult::Affected(n as usize),
                _ => unreachable!(),
            },
            _ => QueryResult::Rows {
                schema: plan.schema().clone(),
                rows,
            },
        };

        Ok(result)
    }
}
