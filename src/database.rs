use std::{io, path::Path, sync::Arc};

use crate::catalog::{self, Catalog, Schema};
use crate::exec::{self, Context};
use crate::plan::{Plan, binder};
use crate::sql::{self, ast::Stmt};
use crate::storage::{DEFAULT_POOL_CAPACITY, Disk, FileDisk, MemDisk, Pool};
use crate::value::{Row, Value};

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
    Exec(exec::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum QueryResult {
    None,
    Affected(usize),
    Rows { rows: Vec<Row>, schema: Arc<Schema> },
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
            Stmt::Update(update) => {
                let plan = binder::bind_update(&self.cata, &update)?;
                self.run(&plan)
            }
            Stmt::Delete(delete) => {
                let plan = binder::bind_delete(&self.cata, &delete)?;
                self.run(&plan)
            }
        }
    }

    fn run(&mut self, plan: &Plan) -> Result<QueryResult> {
        println!("{plan:#?}");
        let mut root = exec::build::<D>(plan, &self.cata);
        let mut cx = Context::new(&mut self.pool, &mut self.cata);

        let mut rows = Vec::new();
        while let Some(tuple) = root.next(&mut cx)? {
            rows.push(tuple.row)
        }

        Ok(match cx.rows_affected {
            Some(n) => QueryResult::Affected(n),
            None => QueryResult::Rows {
                rows,
                schema: plan.schema().clone(),
            },
        })
    }
}

macro_rules! sql {
    ($($arg:tt)*) => {
        sql(format_args!($($arg)*))
    };
}

#[derive(Debug)]
enum Sql<'a> {
    Owned(String),
    Borrowed(&'a str),
}

fn sql<'a>(args: std::fmt::Arguments<'a>) -> Sql<'a> {
    match args.as_str() {
        None => Sql::Owned(args.to_string()),
        Some(s) => Sql::Borrowed(s),
    }
}

impl Sql<'_> {
    fn execute(self, db: &mut Database<impl Disk>) -> Result<QueryResult> {
        match self {
            Sql::Owned(input) => db.execute(&input),
            Sql::Borrowed(input) => db.execute(input),
        }
    }

    fn execute_batch(self, db: &mut Database<impl Disk>) -> Result<Vec<QueryResult>> {
        match self {
            Sql::Owned(input) => db.execute_batch(&input),
            Sql::Borrowed(input) => db.execute_batch(input),
        }
    }
}

impl std::fmt::Display for QueryResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryResult::None => Ok(()),
            QueryResult::Affected(n) => write!(f, "{n} row(s) affected"),
            QueryResult::Rows { rows, schema } => {
                if rows.len() != 0 {
                    writeln!(f, "{}", Tabular::new(schema, rows))?;
                }
                write!(f, "({} rows)", rows.len())
            }
        }
    }
}

struct Tabular<'a> {
    schema: &'a Schema,
    tuples: &'a [Row],
}

impl<'a> Tabular<'a> {
    fn new(schema: &'a Schema, tuples: &'a [Row]) -> Self {
        Self { schema, tuples }
    }
}

impl std::fmt::Display for Tabular<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut b = tabled::builder::Builder::new();
        b.push_record(self.schema.columns.iter().map(|col| col.name.as_ref()));
        self.tuples
            .iter()
            .for_each(|row| b.push_record(row.iter().map(|val| val.to_string())));
        write!(f, "{}", b.build())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const POOL_CAP: usize = 50;

    fn mem_db() -> Database<MemDisk> {
        Database::with_disk(MemDisk::default(), POOL_CAP).unwrap()
    }

    fn temp_db() -> Database<FileDisk> {
        Database::with_disk(FileDisk::temp().unwrap(), POOL_CAP).unwrap()
    }

    struct Timeit(Instant);

    impl Drop for Timeit {
        fn drop(&mut self) {
            println!("took {:?}", self.0.elapsed());
        }
    }

    macro_rules! timeit {
        ($($arg:tt)*) => {{
            let _span = Timeit(Instant::now());
            $($arg)*
        }};
    }

    fn bulk_insert(db: &mut Database<impl Disk>, count: usize) {
        for n in 1..=count {
            sql!("insert into t values ('foo', {n}, true)")
                .execute(db)
                .unwrap();
        }
    }

    #[test]
    fn persist() -> Result<()> {
        let mut db = temp_db();
        sql!("create table t (s text, n int, b bool)").execute(&mut db)?;
        timeit! {
            bulk_insert(&mut db, 20);
        }
        Ok(())
    }
}
