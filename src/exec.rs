use std::{io, sync::Arc};

use crate::catalog::{Catalog, Schema};
use crate::heap::HeapScan;
use crate::plan::{BoundExpr, EvalError, Plan};
use crate::storage::{Disk, Pool};
use crate::value::{Value, codec};

pub type Tuple = Vec<Value>;

pub trait Operator<D: Disk> {
    fn next(&mut self, pool: &mut Pool<D>) -> Result<Option<Tuple>, Error>;
}

pub fn build<'p, D: Disk + 'p>(plan: &'p Plan, cata: &Catalog) -> Box<dyn Operator<D> + 'p> {
    match plan {
        Plan::OneRow => Box::new(OneRow { done: false }),
        Plan::Scan { table, schema } => {
            let cursor = cata.get(&table).expect("bound").heap.scan();
            Box::new(SeqScan {
                cursor,
                schema: schema.clone(),
            })
        }
        Plan::Project {
            input, projection, ..
        } => Box::new(Project {
            input: build(input, cata),
            projection,
        }),
    }
}

#[derive(Debug, derive_more::From)]
pub enum Error {
    #[from]
    Io(io::Error),
    #[from]
    Eval(EvalError),
}

// type Result<T> = std::result::Result<T, Error>;

struct OneRow {
    done: bool,
}

impl<D: Disk> Operator<D> for OneRow {
    fn next(&mut self, _: &mut Pool<D>) -> Result<Option<Tuple>, Error> {
        Ok(if std::mem::replace(&mut self.done, true) { None } else { Some(vec![]) })
    }
}

struct SeqScan {
    cursor: HeapScan,
    schema: Arc<Schema>,
}

impl<D: Disk> Operator<D> for SeqScan {
    fn next(&mut self, pool: &mut Pool<D>) -> Result<Option<Tuple>, Error> {
        match self.cursor.next(pool)? {
            None => Ok(None),
            Some((_rid, record)) => Ok(Some(codec::decode(&self.schema, record))),
        }
    }
}

struct Project<'p, D: Disk> {
    input: Box<dyn Operator<D> + 'p>,
    projection: &'p [BoundExpr],
}

impl<D: Disk> Operator<D> for Project<'_, D> {
    fn next(&mut self, pool: &mut Pool<D>) -> Result<Option<Tuple>, Error> {
        let Some(tuple) = self.input.next(pool)? else {
            return Ok(None);
        };
        let out = self
            .projection
            .iter()
            .map(|expr| expr.eval(&tuple))
            .collect::<Result<_, _>>()?;
        Ok(Some(out))
    }
}
