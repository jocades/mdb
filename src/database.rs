use std::sync::Arc;
use std::{io, path::Path};

use crate::catalog::{self, Catalog, Schema};
use crate::exec::Tuple;
use crate::exec::{self, Context};
use crate::plan::{Plan, binder};
use crate::sql::{self, ast::Stmt};
use crate::storage::{DEFAULT_POOL_CAPACITY, Disk, FileDisk, MemDisk, Pool};
use crate::value::Value;

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
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum QueryResult {
    None,
    Affected(usize),
    Rows {
        rows: Vec<Tuple>,
        schema: Arc<Schema>,
    },
}

impl<D: Disk> Database<D> {
    pub fn execute(&mut self, sql: &str) -> Result<QueryResult> {
        let stmt = sql::parse_one(sql)?;
        self.execute_stmt(stmt)
    }

    pub fn execute_batch(&mut self, sql: &str) -> Result<Vec<QueryResult>> {
        sql::parse(sql)?
            .into_iter()
            .map(|stmt| self.execute_stmt(stmt))
            .collect()
    }

    fn execute_stmt(&mut self, stmt: Stmt) -> Result<QueryResult> {
        match stmt {
            Stmt::CreateTable { name, defs } => {
                let (name, schema) = binder::bind_create_table(&self.cata, &name, &defs)?;
                self.cata.create_table(&mut self.pool, name, schema)?;
                Ok(QueryResult::None)
            }
            Stmt::DropTable { name } => {
                ensure!(
                    self.cata.get(&name.lexeme).is_some(),
                    binder::Error::TableNotFound(name)
                );
                self.cata.drop_table(&mut self.pool, &name.lexeme)?;
                Ok(QueryResult::None)
            }
            Stmt::Insert(insert) => {
                let plan = binder::bind_insert(&self.cata, &insert)?;
                self.run(&plan)
            }
            Stmt::Select(select) => {
                let plan = binder::bind_select(&self.cata, &select)?;
                self.run(&plan)
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const POOL_CAP: usize = 5;

    fn mem_db() -> Database<MemDisk> {
        Database::with_disk(MemDisk::default(), POOL_CAP).unwrap()
    }

    #[test]
    fn bulk_insert() -> Result<()> {
        let mut db = mem_db();
        db.execute("create table t (s text, n int, b bool)")?;
        for _ in 1..=200 {
            db.execute("insert into f values ('foo', {n}, true)")?;
        }
        Ok(())
    }
}
