use std::collections::HashMap;
use std::sync::Arc;

use crate::catalog::{self, Catalog, Column, Schema};
use crate::sql::ast::{BinOp, ColumnDef, Expr, Ident};
use crate::value::{Type, Value};

use super::Plan;
use super::bound::BoundExpr;

#[derive(Debug)]
pub enum Error {
    TableExists(Ident),
    DuplicateColumn(Ident),
    TableNotFound(Ident),
    ColumnNotFound(Ident),
    InvalidOperands { op: BinOp, lty: Type, rty: Type },
}

type Result<T> = std::result::Result<T, Error>;

/// Validate table creation
pub fn bind_create_table(
    cata: &Catalog,
    name: &Ident,
    defs: &[ColumnDef],
) -> Result<(Arc<str>, Schema)> {
    ensure!(
        cata.get(&name.lexeme).is_none(),
        Error::TableExists(name.clone())
    );

    let mut columns = Vec::with_capacity(defs.len());
    let mut by_name = HashMap::with_capacity(defs.len());
    for (idx, col) in defs.iter().enumerate() {
        if let Some(_existing) = by_name.insert(col.name.lexeme.clone(), idx) {
            bail!(Error::DuplicateColumn(col.name.clone()))
        }
        columns.push(Column::new(col.name.lexeme.clone(), col.ty));
    }

    Ok((name.lexeme.clone(), Schema { columns, by_name }))
}

pub fn bind_expr(expr: &Expr, input: &Schema) -> Result<BoundExpr> {
    match expr {
        Expr::Lit(lit) => Ok(BoundExpr::Lit(lit.into())),
        Expr::Ident(name) => {
            let index = input
                .index_of(&name.lexeme)
                .ok_or_else(|| Error::ColumnNotFound(name.clone()))?;

            Ok(BoundExpr::Column {
                index,
                ty: input.columns[index].ty,
            })
        }
        Expr::Bin(lhs, op, rhs) => {
            let (lhs, rhs) = (bind_expr(lhs, input)?, bind_expr(rhs, input)?);
            let (lty, rty) = (lhs.ty(), rhs.ty());

            let op = *op;
            let ty = type_of_bin(op, lty, rty).ok_or(Error::InvalidOperands { op, lty, rty })?;

            Ok(BoundExpr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                ty,
            })
        }
        Expr::Wildcard => unreachable!("expanded by the select binder"),
    }
}

fn type_of_bin(op: BinOp, lhs: Type, rhs: Type) -> Option<Type> {
    use BinOp::*;
    match (op, lhs, rhs) {
        (Add | Sub | Mul | Div, Type::Int, Type::Int) => Some(Type::Int),
        (Eq | Ne | Gt | Ge | Lt | Le, a, b) if a == b => Some(Type::Bool),
        _ => None,
    }
}

pub fn bind_select(cata: &Catalog, exprs: &[Expr], from: &Option<Ident>) -> Result<Plan> {
    let input = match from {
        None => Plan::OneRow,
        Some(name) => {
            let table = cata
                .get(&name.lexeme)
                .ok_or_else(|| Error::TableNotFound(name.clone()))?;
            Plan::Scan {
                table: table.name.clone(),
                schema: table.schema.clone(),
            }
        }
    };

    let mut projection = Vec::new();
    let mut cols = Vec::new();
    for expr in exprs {
        match expr {
            Expr::Wildcard => {
                for (index, col) in input.schema().columns.iter().enumerate() {
                    projection.push(BoundExpr::Column { index, ty: col.ty });
                    cols.push(Column::new(col.name.clone(), col.ty));
                }
            }
            _ => {
                let bound = bind_expr(expr, &input.schema())?;
                let name = match expr {
                    Expr::Ident(ident) => ident.lexeme.clone(),
                    _ => "?column?".into(),
                };
                cols.push(Column::new(name, bound.ty()));
                projection.push(bound);
            }
        }
    }

    Ok(Plan::Project {
        input: Box::new(input),
        projection,
        schema: Arc::new(Schema::new(cols)),
    })
}
