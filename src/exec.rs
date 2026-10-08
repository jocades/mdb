use std::{io, mem};

use crate::catalog::{self, Catalog, Schema};
use crate::heap::HeapScan;
use crate::plan::{BoundExpr, EvalError, Plan};
use crate::storage::{Disk, Pool};
use crate::value::{Value, codec};

pub type Tuple = Vec<Value>;

pub struct Context<'a, P: Disk> {
    pub pool: &'a mut Pool<P>,
    pub cata: &'a mut Catalog,
}

pub trait Operator<D: Disk> {
    fn next(&mut self, cx: &mut Context<D>) -> Result<Option<Tuple>, Error>;
}

pub fn build<'p, D: Disk + 'p>(plan: &'p Plan, cata: &Catalog) -> Box<dyn Operator<D> + 'p> {
    match plan {
        Plan::OneRow => Box::new(OneRow { done: false }),
        Plan::Scan { table, schema } => {
            let cursor = cata.get(&table).expect("bound").heap.scan();
            Box::new(SeqScan { cursor, schema })
        }
        Plan::Project {
            input, projection, ..
        } => Box::new(Project {
            input: build(input, cata),
            projection,
        }),
        Plan::Values { exprs } => Box::new(Values { exprs, index: 0 }),
        Plan::Insert { table, input, .. } => Box::new(Insert {
            tid: table,
            input: build(input, cata),
            done: false,
        }),
    }
}

#[derive(Debug, derive_more::From)]
pub enum Error {
    #[from]
    Io(io::Error),
    #[from]
    Eval(EvalError),
    #[from]
    Catalog(catalog::Error),
}

// type Result<T> = std::result::Result<T, Error>;

struct OneRow {
    done: bool,
}

impl<D: Disk> Operator<D> for OneRow {
    fn next(&mut self, _: &mut Context<'_, D>) -> Result<Option<Tuple>, Error> {
        Ok(if mem::replace(&mut self.done, true) { None } else { Some(vec![]) })
    }
}

struct SeqScan<'p> {
    cursor: HeapScan,
    schema: &'p Schema,
}

impl<D: Disk> Operator<D> for SeqScan<'_> {
    fn next(&mut self, cx: &mut Context<D>) -> Result<Option<Tuple>, Error> {
        match self.cursor.next(&mut cx.pool)? {
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
    fn next(&mut self, pool: &mut Context<D>) -> Result<Option<Tuple>, Error> {
        let Some(tuple) = self.input.next(pool)? else {
            return Ok(None);
        };
        Ok(Some(
            self.projection
                .iter()
                .map(|expr| expr.eval(&tuple))
                .collect::<Result<_, _>>()?,
        ))
    }
}

struct Values<'p> {
    exprs: &'p [Vec<BoundExpr>],
    index: usize,
}

impl<D: Disk> Operator<D> for Values<'_> {
    fn next(&mut self, _: &mut Context<D>) -> Result<Option<Tuple>, Error> {
        let Some(exprs) = self.exprs.get(self.index) else {
            return Ok(None);
        };
        self.index += 1;
        let tuple = exprs
            .iter()
            .map(|expr| expr.eval(&[]))
            .collect::<Result<_, _>>()?;
        Ok(Some(tuple))
    }
}

struct Insert<'p, D> {
    tid: &'p str,
    input: Box<dyn Operator<D> + 'p>,
    done: bool,
}

impl<D: Disk> Operator<D> for Insert<'_, D> {
    fn next(&mut self, cx: &mut Context<D>) -> Result<Option<Tuple>, Error> {
        if mem::replace(&mut self.done, true) {
            return Ok(None);
        }

        // Drain the child, this should be revisited but for now its fine to keep it in memory
        // it also avoids the `halloween problem` entirely but at a cost...
        let mut tuples = Vec::new();
        while let Some(tuple) = self.input.next(cx)? {
            tuples.push(tuple);
        }

        let table = cx.cata.get_mut(self.tid).expect("bound");
        for tuple in &tuples {
            table.insert(&mut cx.pool, tuple)?;
        }

        // hacky way to return affected rows
        Ok(Some(vec![Value::Int(tuples.len() as i64)]))
    }
}
