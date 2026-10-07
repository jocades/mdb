pub mod binder;
mod bound;

use std::sync::Arc;

use crate::catalog::Schema;
pub use bound::{BoundExpr, EvalError};

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
}

impl Plan {
    pub fn schema(&self) -> Arc<Schema> {
        match self {
            Plan::OneRow => Arc::new(Schema::empty()),
            Plan::Scan { schema, .. } | Plan::Project { schema, .. } => schema.clone(),
        }
    }
}
