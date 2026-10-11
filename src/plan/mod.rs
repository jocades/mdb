pub mod binder;
use binder::BoundExpr;

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
    Update {
        tid: Arc<str>,
        sets: Vec<(usize, BoundExpr)>,
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
            Plan::Filter { child, .. }
            | Plan::Delete { child, .. }
            | Plan::Update { child, .. } => child.schema(),
            Plan::Scan { schema, .. }
            | Plan::Project { schema, .. }
            | Plan::Insert { schema, .. }
            | Plan::Values { schema, .. } => schema,
        }
    }
}

fn display(node: &Plan, f: &mut std::fmt::Formatter<'_>, d: usize) -> std::fmt::Result {
    macro_rules! wln {
        ($b:expr, $depth:expr, $($args:tt)*) => {{
            write!($b, "{:>width$}", "", width = $depth * 2)?;
            writeln!($b, $($args)*)?;
        }};
    }
    match node {
        Plan::OneRow => wln!(f, d, "OneRow"),
        Plan::Scan { tid, .. } => {
            wln!(f, d, "SeqScan({tid})");
        }
        Plan::Filter { child, predicate } => {
            wln!(f, d, "Filter({predicate:?})");
            display(child, f, d + 1)?;
        }
        Plan::Project { child, schema, .. } => {
            let p = schema
                .columns
                .iter()
                .map(|col| col.name.as_ref())
                .collect::<Vec<_>>()
                .join(",");
            wln!(f, d, "Project({p})");
            display(child, f, d + 1)?;
        }
        Plan::Values { rows, .. } => {
            let vs = rows
                .iter()
                .map(|row| format!("{row:?}"))
                .collect::<Vec<_>>()
                .join(",");
            wln!(f, d, "Values({vs})");
        }
        Plan::Insert { tid, child, .. } => {
            wln!(f, d, "Insert({tid})");
            display(child, f, d + 1)?;
        }
        Plan::Delete { tid, child, .. } => {
            wln!(f, d, "Delete({tid})");
            display(child, f, d + 1)?;
        }
        Plan::Update { tid, child, .. } => {
            wln!(f, d, "Update({tid})");
            display(child, f, d + 1)?;
        }
    }
    Ok(())
}

impl std::fmt::Display for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        display(self, f, 0)
    }
}
