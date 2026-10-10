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

// The plan must outlive the operator tree
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
        Ok(None)
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
        Ok(None)
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
            // for (index, expr) in self.sets {
            //     tuple.row[*index] = eval(expr, &tuple.row)?;
            // }

            let new_values = self
                .sets
                .iter()
                .map(|(_, expr)| eval(expr, &tuple.row))
                .collect::<Result<Row, _>>()?;

            for ((index, _), value) in self.sets.iter().zip(new_values) {
                tuple.row[*index] = value;
            }

            updates.push(tuple);
        }

        let table = cx.cata.get_mut(self.tid).expect("bound");
        let affected = updates.len();
        for tuple in updates {
            let record = codec::encode(&table.schema, &tuple.row);
            // todo: when we implement indices we must update the btree if the row has moved
            let _new_rid = table
                .heap
                .update(&mut cx.pool, tuple.rid.expect("planned"), &record)?;
        }

        cx.rows_affected = Some(affected);
        Ok(None)
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

/*
# Per-row allocation in the executor

## The problem

Assuming the page we need is already cached in the pool (so no I/O), row
throughput is limited by per-row allocation.

For every row a scan produces:

1. The page is borrowed from the pool (no copy, no allocation).
2. `codec::decode` allocates a fresh `Vec<Value>` for the row.
3. Every text column allocates again to build an `Arc<str>`.
4. Every column is decoded, even if the query never reads it.

Cloning an `Arc<str>` is cheap, so constants are not the issue. The cost is
the fresh allocations made while decoding, repeated for every row.

## Improvements, from cheapest to most invasive

Each step lists what it removes. None of them has a benchmark yet.
Add one per step and keep the baseline, so every change shows a measured delta.

### 1. Reuse the row buffer (removes the `Vec` per row)

Decode into a buffer that is cleared and refilled, so capacity is kept
after the first row. There are two ways to hand operators that buffer:

- **Out-param**: the caller owns the tuple and passes it down.
  `fn next(&mut self, cx: &mut Context, out: &mut Tuple) -> Result<bool>`
  Pass-through operators (`Filter`) reuse the same `out`. `Project` needs
  one scratch tuple for its child, since it reads the child's row while
  building its own.
- **Cursor** (preferred): each row-producing operator owns its tuple.
  `fn advance(&mut self, cx: &mut Context) -> Result<bool>` steps forward,
  and `fn tuple(&self) -> &Tuple` exposes the current row.
  Only operators that create row contents own a buffer (scan, project,
  values). `Filter` and `Limit` just forward `child.tuple()`.
  It also gives the streaming `Rows::next` API for free: call `advance`,
  then return `tuple()`.

Blocking operators (`Insert`, `Delete`) need a buffer per operator, not
per row. `Delete` collects a `Vec<Rid>`. `Insert` can encode all records
into one contiguous `Vec<u8>` plus a `Vec<(u32, u32)>` of offsets.

### 2. Strings that don't allocate

Most text values are short, so keep them inline and only allocate for long
ones. Crates such as `CompactString` do this, but a minimal version is easy:

```ignore
enum Text {
    // Copying at most 22 bytes is negligible next to an allocation.
    Inline { buf: [u8; 22], len: u8 },
    Alloc(Arc<str>),
}
```

Decoding short strings then allocates nothing, and cloning them is a plain copy.

### 3. Decode only what the query needs (column pruning)

The binder knows which column indices a query touches. Pass them to the
scan, and have `decode` skip the rest by reading lengths and advancing.
Skipping text costs a length read and no allocation. `SELECT *` gains
nothing, but wide tables with narrow projections gain a lot.

### 4. Lazy decoding

Don't build a `Vec<Value>` at all. The scan's tuple holds the raw record
bytes (copied once into a reused buffer) plus column offsets, and a column
is decoded only when an expression reads it.

- Unused columns cost nothing, so pruning becomes automatic.
- `WHERE name = 'bob'` can compare bytes directly without building a string.
- Costs: `eval` must read through the tuple instead of `&[Value]`, the cache
  needs `&mut` access or interior mutability, and finding column N means
  walking the earlier fields unless the record format gets an offset table
  (a file format change).

Because `tuple()` is the single point where rows are exposed, steps 1 to 4
can be swapped in without changing `advance`, the plan, or the streaming API.

### 5. Per-row or per-statement arena (only for some operators)

Allocations go into a bump region that is reset per row, or dropped at the
end of the statement. This doesn't help plain scans, filters and projections.
It helps operators that create many temporaries or hold many values:
sort buffers, hash join tables, aggregate state, string functions.
The cost is `Value<'a>` lifetimes, and anything that must outlive the row
has to be copied out. Wait until one of those operators exists.

### 6. Batches (vectorized execution)

`next_batch` returns about 1024 rows in columnar vectors, and buffers are
reused across batches. Allocation and virtual dispatch are amortized over
the batch, and loops become tight. It is the largest speedup, but it is a
different executor: expressions work over vectors, filters produce
selection vectors, and strings need a view representation.

### Not planned: push-based fused pipelines

They give the best fusion, but they invert control flow and conflict with
the pull-based `Rows` stream and early-exit `LIMIT`.

## Benchmarks to write (TODO)

Use `criterion` on `MemDisk` with a pool large enough to hold the table,
and count allocations with a wrapping global allocator.

| Step | Benchmark | Expected effect |
|------|-----------|-----------------|
| baseline | `SELECT *` over narrow and wide tables | reference rows/sec and allocs/row |
| 1 | same, with reused buffers | allocs/row drops by about one |
| 2 | wide table with short text | text allocs/row go to about 0 |
| 3 | `SELECT id` over a wide table | time drops with the width of skipped columns |
| 4 | selective `WHERE` on an early column | decode cost proportional to columns read |
| 5 | sort / hash join (once they exist) | fewer allocs, cheaper teardown |
| 6 | scan + filter + project | rows/sec, compared against the row-at-a-time baseline |
*/
