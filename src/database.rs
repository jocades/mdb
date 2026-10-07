use std::sync::Arc;
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
    #[from]
    Catalog(catalog::Error),
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
    ColumnTypeMismatch {
        column: Arc<str>,
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
                    self.catalog.create_table(&mut self.pool, name, columns)?;
                }
                Stmt::DropTable { name } => self.catalog.drop_table(&mut self.pool, name)?,
                Stmt::Insert {
                    into,
                    values: exprs,
                } => {
                    let Some(table) = self.catalog.get_mut(&into.lexeme) else {
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
                            Error::ColumnTypeMismatch {
                                column: col.name.clone(),
                                expected: col.ty,
                                found: ty,
                            }
                        );
                        tuple.push(value);
                    }

                    table.insert(&mut self.pool, &tuple).unwrap();
                }
                Stmt::Select {
                    projection,
                    relation,
                } => match relation {
                    None => {
                        let mut result = Vec::with_capacity(projection.len());
                        for expr in &projection {
                            let value = eval(expr);
                            result.push(value);
                        }
                        dbg!(result);
                    }
                    Some(name) => {
                        let Some(table) = self.catalog.tables.get(&name.lexeme) else {
                            bail!(Error::TableNotFound(name));
                        };

                        for expr in &projection {
                            match expr {
                                Expr::Ident(col) => {
                                    ensure!(
                                        table.schema.by_name.contains_key(&col.lexeme),
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

// enum EvalError {
//     InvalidOperands {
//         lhs: Type,
//         rhs: Type,
//     }
// }

fn eval(expr: &Expr) -> Value {
    use crate::sql::ast::BinOp;
    match expr {
        Expr::Lit(lit) => lit.into(),
        Expr::Ident(_) => todo!(),
        Expr::Bin(lhs, op, rhs) => {
            let lhs = eval(lhs);
            let rhs = eval(rhs);
            match op {
                BinOp::Add => match (lhs, rhs) {
                    (Value::Int(x), Value::Int(y)) => Value::Int(x + y),
                    _ => panic!("invalid operands"),
                },
                BinOp::Sub => match (lhs, rhs) {
                    (Value::Int(x), Value::Int(y)) => Value::Int(x - y),
                    _ => panic!("invalid operands"),
                },
                BinOp::Mul => todo!(),
                BinOp::Div => todo!(),

                BinOp::Eq => todo!(),
                BinOp::Ne => todo!(),
                BinOp::Gt => todo!(),
                BinOp::Ge => todo!(),
                BinOp::Lt => todo!(),
                BinOp::Le => todo!(),
            }
        }
        Expr::Wildcard => todo!(),
    }
}

struct Context<'a, D: Disk> {
    pool: &'a mut Pool<D>,
    catalog: &'a Catalog,
}
