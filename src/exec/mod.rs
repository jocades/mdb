mod eval;

use std::{io, mem};

use crate::catalog::{self, Catalog, Schema};
use crate::heap::{HeapScan, Rid};
use crate::plan::{BoundExpr, Plan};
use crate::storage::{Disk, Pool};
use crate::value::{Row, Value, codec};

pub use eval::{EvalError, eval};

pub struct Context<'a, D: Disk> {
    pub pool: &'a mut Pool<D>,
    pub cata: &'a mut Catalog,
    pub rows_affected: Option<usize>,
}

impl<'a, D: Disk> Context<'a, D> {
    pub fn new(pool: &'a mut Pool<D>, cata: &'a mut Catalog) -> Self {
        Self {
            pool,
            cata,
            rows_affected: None,
        }
    }
}

#[derive(Debug)]
pub struct Tuple {
    pub row: Row,
    pub rid: Option<Rid>,
}

impl Tuple {
    pub fn new(row: Row) -> Self {
        Self { row, rid: None }
    }

    pub fn with_rid(row: Row, rid: Rid) -> Self {
        Self {
            row,
            rid: Some(rid),
        }
    }

    pub fn empty() -> Self {
        Self {
            row: vec![],
            rid: None,
        }
    }
}

pub trait Operator<D: Disk> {
    fn next(&mut self, cx: &mut Context<D>) -> Result<Option<Tuple>, Error>;
}

pub fn build<'p, D: Disk + 'p>(plan: &'p Plan, cata: &Catalog) -> Box<dyn Operator<D> + 'p> {
    match plan {
        Plan::OneRow => Box::new(OneRow { done: false }),
        Plan::Scan { tid, schema } => {
            let cursor = cata.get(&tid).expect("bound").heap.scan();
            Box::new(SeqScan { cursor, schema })
        }
        Plan::Project {
            child, projection, ..
        } => Box::new(Project {
            child: build(child, cata),
            projection,
        }),
        Plan::Values { rows: exprs, .. } => Box::new(Values { exprs, index: 0 }),
        Plan::Insert { tid, child, .. } => Box::new(Insert {
            tid,
            child: build(child, cata),
            done: false,
        }),
        Plan::Filter { child, predicate } => Box::new(Filter {
            child: build(child, cata),
            predicate,
        }),
        Plan::Delete { tid, child } => Box::new(Delete {
            child: build(child, cata),
            tid,
            done: false,
        }),
        Plan::Update { tid, sets, child } => Box::new(Update {
            tid,
            sets,
            child: build(child, cata),
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
}

struct OneRow {
    done: bool,
}

impl<D: Disk> Operator<D> for OneRow {
    fn next(&mut self, _: &mut Context<'_, D>) -> Result<Option<Tuple>, Error> {
        Ok(if mem::replace(&mut self.done, true) { None } else { Some(Tuple::empty()) })
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
            Some((rid, record)) => {
                let row = codec::decode(&self.schema, record);
                Ok(Some(Tuple::with_rid(row, rid)))
            }
        }
    }
}

struct Filter<'p, D: Disk> {
    child: Box<dyn Operator<D> + 'p>,
    predicate: &'p BoundExpr,
}

impl<D: Disk> Operator<D> for Filter<'_, D> {
    fn next(&mut self, cx: &mut Context<D>) -> Result<Option<Tuple>, Error> {
        while let Some(tuple) = self.child.next(cx)? {
            if eval(self.predicate, &tuple.row)? == Value::Bool(true) {
                return Ok(Some(tuple));
            }
        }
        Ok(None)
    }
}

struct Project<'p, D: Disk> {
    child: Box<dyn Operator<D> + 'p>,
    projection: &'p [BoundExpr],
}

impl<D: Disk> Operator<D> for Project<'_, D> {
    fn next(&mut self, cx: &mut Context<D>) -> Result<Option<Tuple>, Error> {
        let Some(tuple) = self.child.next(cx)? else {
            return Ok(None);
        };

        let row = self
            .projection
            .iter()
            .map(|expr| eval(expr, &tuple.row))
            .collect::<Result<Row, _>>()?;

        Ok(Some(Tuple::new(row)))
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
        let rows = exprs
            .iter()
            .map(|expr| eval(expr, &[]))
            .collect::<Result<Row, _>>()?;
        Ok(Some(Tuple::new(rows)))
    }
}

struct Insert<'p, D> {
    tid: &'p str,
    child: Box<dyn Operator<D> + 'p>,
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
        while let Some(tuple) = self.child.next(cx)? {
            tuples.push(tuple);
        }

        let table = cx.cata.get_mut(self.tid).expect("bound");
        for tuple in &tuples {
            table.insert(&mut cx.pool, &tuple.row)?;
        }

        cx.rows_affected = Some(tuples.len());
        Ok(Some(Tuple::empty()))
    }
}

struct Delete<'p, D> {
    tid: &'p str,
    child: Box<dyn Operator<D> + 'p>,
    done: bool,
}

impl<D: Disk> Operator<D> for Delete<'_, D> {
    fn next(&mut self, cx: &mut Context<D>) -> Result<Option<Tuple>, Error> {
        if mem::replace(&mut self.done, true) {
            return Ok(None);
        }

        let mut rids = Vec::new();
        while let Some(tuple) = self.child.next(cx)? {
            rids.push(tuple.rid.expect("planned"));
        }

        let table = cx.cata.get_mut(self.tid).expect("bound");
        let affected = rids.len();
        for rid in rids {
            table.heap.delete(&mut cx.pool, rid)?;
        }

        cx.rows_affected = Some(affected);
        Ok(Some(Tuple::empty()))
    }
}

struct Update<'p, D> {
    tid: &'p str,
    child: Box<dyn Operator<D> + 'p>,
    sets: &'p [(usize, BoundExpr)],
    done: bool,
}

impl<D: Disk> Operator<D> for Update<'_, D> {
    fn next(&mut self, cx: &mut Context<D>) -> Result<Option<Tuple>, Error> {
        if mem::replace(&mut self.done, true) {
            return Ok(None);
        }

        let mut updates = Vec::new();
        while let Some(mut tuple) = self.child.next(cx)? {
            for (index, expr) in self.sets {
                tuple.row[*index] = eval(expr, &tuple.row)?;
            }
            updates.push(tuple);
        }

        let table = cx.cata.get_mut(self.tid).expect("bound");
        let affected = updates.len();
        for tuple in updates {
            let record = codec::encode(&table.schema, &tuple.row);
            table
                .heap
                .update(&mut cx.pool, tuple.rid.expect("planned"), &record)?;
        }

        cx.rows_affected = Some(affected);
        Ok(Some(Tuple::empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::sql;
    use crate::sql::ast::BinOp;
    use crate::storage::{MemDisk, Pool};
    use crate::value::{Type, Value};

    #[test]
    fn simple_select_with_filter() -> Result<(), crate::database::Error> {
        // t = { x: int }
        // select x from t where y = 4;
        //
        // Project([x])
        //   Filter(y = 4)
        //     SeqScan(t)

        let mut db = Database::memory();
        db.execute("create table t (x int, y int)")?;
        db.execute("insert into t values (1, 2), (3, 4)")?;

        let table = db.cata.get("t").unwrap();

        let plan_schema = table.schema.clone();
        let scan = table.heap.scan();

        // the binder would create this tree
        let seqscan = Box::new(SeqScan {
            schema: plan_schema.as_ref(),
            cursor: scan,
        });

        let predicate = BoundExpr::Bin {
            op: BinOp::Eq,
            lhs: Box::new(BoundExpr::Column {
                index: 1,
                ty: Type::Int,
            }),
            rhs: Box::new(BoundExpr::Const(Value::Int(4))),
            ty: Type::Bool,
        };

        let filter = Box::new(Filter {
            child: seqscan,
            predicate: &predicate,
        });

        let mut root: Box<dyn Operator<MemDisk>> = Box::new(Project {
            child: filter,
            projection: &[BoundExpr::Column {
                index: 0,
                ty: Type::Int,
            }],
        });

        let mut cx = Context::new(&mut db.pool, &mut db.cata);

        let mut rows = Vec::new();
        while let Some(tuple) = root.next(&mut cx)? {
            rows.push(tuple);
        }

        println!("{:?}", plan_schema.columns);
        println!("{rows:?}");

        assert_eq!(rows.len(), 1);
        // assert_eq!(rows[0][0], Value::Int(3));

        Ok(())
    }
}
