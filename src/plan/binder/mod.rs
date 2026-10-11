mod ddl;
mod dml;
mod expr;
mod select;

pub use expr::BoundExpr;

use crate::catalog::{Catalog, Schema};
use crate::plan::Plan;
use crate::sql::ast::{BinOp, Ident, Stmt};
use crate::value::Type;
use std::sync::Arc;

pub enum Bound {
    /// DDL
    Util(Util),
    /// DML
    Query(Plan),
}

pub enum Util {
    CreateTable { name: Arc<str>, schema: Schema },
    DropTable { name: Arc<str> },
}

pub fn bind(cata: &Catalog, stmt: Stmt) -> Result<Bound> {
    Ok(match stmt {
        Stmt::CreateTable { name, defs } => Bound::Util(ddl::create_table(cata, name, defs)?),
        Stmt::DropTable { name } => Bound::Util(ddl::drop_table(cata, name)?),
        stmt => {
            let plan = Binder { cata }.bind(stmt)?;
            Bound::Query(plan)
        }
    })
}

struct Binder<'c> {
    cata: &'c Catalog,
}

impl Binder<'_> {
    fn bind(&self, stmt: Stmt) -> Result<Plan> {
        match stmt {
            Stmt::Insert(insert) => self.insert(insert),
            Stmt::Select(select) => self.select(select),
            Stmt::Update(update) => self.update(update),
            Stmt::Delete(delete) => self.delete(delete),
            _ => unreachable!("ddl"),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    TableExists(Ident),
    DuplicateColumn(Ident),
    TableNotFound(Ident),
    ColumnNotFound(Ident),
    FunctionNotFound(Ident),
    FunctionArity {
        name: Ident,
        expected: usize,
        found: usize,
    },
    ArgumentType {
        name: Ident,
        expected: Type,
        found: Type,
    },
    InvalidOperands {
        op: BinOp,
        lty: Type,
        rty: Type,
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
    WhereTypeMustBeBool,
}
