use crate::catalog::EMPTY_SCHEMA;
use crate::plan::Plan;
use crate::sql::ast;

use super::{Binder, Error, Result};

impl Binder<'_> {
    pub fn insert(&self, insert: ast::Insert) -> Result<Plan> {
        let table = self
            .cata
            .get(&insert.into.lexeme)
            .ok_or_else(|| Error::TableNotFound(insert.into.clone()))?;

        let mut vals_list = Vec::with_capacity(insert.vals.len());
        for row in &insert.vals {
            ensure!(
                row.len() == table.schema.columns.len(),
                Error::ArityMismatch {
                    table: insert.into.clone(),
                    expected: table.schema.columns.len() as u16,
                    found: row.len() as u16,
                }
            );

            let mut exprs = Vec::with_capacity(table.schema.columns.len());
            for (expr, col) in row.iter().zip(&table.schema.columns) {
                let bound = self.expr(expr, &EMPTY_SCHEMA)?;
                ensure!(
                    bound.ty() == col.ty,
                    Error::ColumnTypeMismatch {
                        column: col.name.clone(),
                        expected: col.ty,
                        found: bound.ty(),
                    }
                );
                exprs.push(bound);
            }
            vals_list.push(exprs);
        }

        Ok(Plan::Insert {
            tid: table.name.clone(),
            child: Box::new(Plan::Values {
                rows: vals_list,
                schema: table.schema.clone(),
            }),
            schema: EMPTY_SCHEMA.clone(),
        })
    }

    pub fn update(&self, update: ast::Update) -> Result<Plan> {
        let (table, source) = self.scan_with_filter(&update.what, update.were.as_ref())?;

        let scope = source.schema();
        let mut sets = Vec::with_capacity(update.sets.len());
        for (name, expr) in &update.sets {
            let index = scope
                .index_of(&name.lexeme)
                .ok_or_else(|| Error::ColumnNotFound(name.clone()))?;
            let bound = self.expr(expr, scope)?;
            ensure!(
                bound.ty() == scope.columns[index].ty,
                Error::ColumnTypeMismatch {
                    column: scope.columns[index].name.clone(),
                    expected: scope.columns[index].ty,
                    found: bound.ty(),
                }
            );
            sets.push((index, bound));
        }

        Ok(Plan::Update {
            tid: table.name.clone(),
            sets,
            child: Box::new(source),
        })
    }

    pub fn delete(&self, del: ast::Delete) -> Result<Plan> {
        let (table, source) = self.scan_with_filter(&del.from, del.were.as_ref())?;
        Ok(Plan::Delete {
            tid: table.name.clone(),
            child: Box::new(source),
        })
    }
}
