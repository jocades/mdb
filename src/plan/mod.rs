pub mod binder;
mod bound;
pub use bound::{BoundExpr, EvalError};

use crate::catalog::{EMPTY_SCHEMA, Schema};
use std::sync::Arc;

#[derive(Debug)]
pub enum Plan {
    OneRow,
    Scan {
        table: Arc<str>,
        schema: Arc<Schema>,
    },
    Project {
        input: Box<Plan>,
        projection: Vec<BoundExpr>,
        schema: Arc<Schema>,
    },
    Values {
        exprs: Vec<Vec<BoundExpr>>,
    },
    Insert {
        table: Arc<str>,
        input: Box<Plan>,
        schema: Arc<Schema>,
    },
}

impl Plan {
    pub fn schema(&self) -> &Arc<Schema> {
        match self {
            Plan::OneRow | Plan::Values { .. } => &EMPTY_SCHEMA,
            Plan::Scan { schema, .. }
            | Plan::Project { schema, .. }
            | Plan::Insert { schema, .. } => schema,
        }
    }
}
