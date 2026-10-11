use std::sync::Arc;

use crate::catalog::{Schema, Table};
use crate::plan::Plan;
use crate::sql::ast::{self, Expr, Ident};
use crate::value::Type;

use super::expr::BoundExpr;
use super::{Binder, Error, Result};

impl Binder<'_> {
    pub fn select(&self, select: ast::Select) -> Result<Plan> {
        let source = match &select.from {
            None => Plan::OneRow,
            Some(name) => {
                let (_, source) = self.scan(name)?;
                self.filter(source, select.were.as_ref())?
            }
        };

        let mut projection = Vec::new();
        let mut output_schema = Schema::empty(); // built from the projection expressions

        for item in &select.cols {
            match item {
                ast::SelectItem::Wildcard => {
                    for (index, col) in source.schema().columns.iter().enumerate() {
                        projection.push(BoundExpr::Column { index, ty: col.ty });
                        output_schema.add_column(col.name.clone(), col.ty);
                    }
                }
                ast::SelectItem::Expr { expr, .. } => {
                    let bound = self.expr(expr, source.schema())?;
                    let name = match expr {
                        Expr::Ident(ident) => ident.lexeme.clone(),
                        _ => "?column?".into(),
                    };
                    output_schema.add_column(name, bound.ty());
                    projection.push(bound);
                }
            }
        }

        Ok(Plan::Project {
            child: Box::new(source),
            projection,
            schema: Arc::new(output_schema),
        })
    }

    pub fn scan(&self, name: &Ident) -> Result<(&Table, Plan)> {
        let table = self
            .cata
            .get(&name.lexeme)
            .ok_or_else(|| Error::TableNotFound(name.clone()))?;
        let scan = Plan::Scan {
            tid: table.name.clone(),
            schema: table.schema.clone(),
        };
        Ok((table, scan))
    }

    pub fn filter(&self, source: Plan, were: Option<&Expr>) -> Result<Plan> {
        match were {
            None => Ok(source),
            Some(expr) => {
                let predicate = self.expr(expr, source.schema())?;
                ensure!(predicate.ty() == Type::Bool, Error::WhereTypeMustBeBool);
                Ok(Plan::Filter {
                    child: Box::new(source),
                    predicate,
                })
            }
        }
    }

    pub fn scan_with_filter(&self, name: &Ident, were: Option<&Expr>) -> Result<(&Table, Plan)> {
        let (table, source) = self.scan(name)?;
        let plan = self.filter(source, were)?;
        Ok((table, plan))
    }
}
