pub mod binder;
mod bound;
pub use bound::BoundExpr;

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
    Delete {
        tid: Arc<str>,
        child: Box<Plan>,
    },
}

impl Plan {
    /// Get the output schema of this node.
    ///
    /// Every plan node exposes the schema of the rows it produces,
    /// regardless of how those rows are generated.
    pub fn schema(&self) -> &Arc<Schema> {
        match self {
            Plan::OneRow => &EMPTY_SCHEMA,
            Plan::Filter { child, .. } | Plan::Delete { child, .. } => child.schema(),
            Plan::Scan { schema, .. }
            | Plan::Project { schema, .. }
            | Plan::Insert { schema, .. }
            | Plan::Values { schema, .. } => schema,
        }
    }
}
