pub mod binder;
mod bound;
pub use bound::{BoundExpr, EvalError};

use crate::catalog::{EMPTY_SCHEMA, Schema};
use std::sync::Arc;

#[derive(Debug)]
pub enum Plan {
    OneRow,
    Scan {
        tid: Arc<str>,
        schema: Arc<Schema>,
    },
    Project {
        child: Box<Plan>,
        projection: Vec<BoundExpr>,
        schema: Arc<Schema>,
    },
    Values {
        rows: Vec<Vec<BoundExpr>>,
        schema: Arc<Schema>,
    },
    Insert {
        tid: Arc<str>,
        child: Box<Plan>,
        schema: Arc<Schema>,
    },
    Filter {
        child: Box<Plan>,
        predicate: BoundExpr,
    },
}

impl Plan {
    // Each node knows its output schema, which is what the next node binds against
    pub fn schema(&self) -> &Arc<Schema> {
        match self {
            Plan::OneRow | Plan::Values { .. } => &EMPTY_SCHEMA,
            Plan::Filter { child, .. } => child.schema(),
            Plan::Scan { schema, .. }
            | Plan::Project { schema, .. }
            | Plan::Insert { schema, .. } => schema,
        }
    }
}
