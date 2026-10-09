use std::collections::HashSet;
use std::sync::Arc;

use crate::catalog::{Catalog, Column, EMPTY_SCHEMA, Schema};
use crate::sql::ast::{self, BinOp, ColumnDef, Expr, Ident};
use crate::value::Type;

use super::Plan;
use super::bound::BoundExpr;

#[derive(Debug)]
pub enum Error {
    TableExists(Ident),
    DuplicateColumn(Ident),
    TableNotFound(Ident),
    ColumnNotFound(Ident),
    InvalidOperands {
        op: BinOp,
        lty: Type,
        rty: Type,
    },
    ArityMismatch {
        table: Ident,
        expected: u16,
        found: u16,
    },
    ColumnTypeMismatch {
        column: Arc<str>,
        expected: Type,
        found: Type,
    },
    WhereTypeMustBeBool,
}

type Result<T> = std::result::Result<T, Error>;

pub fn bind_create_table(
    cata: &Catalog,
    name: &Ident,
    defs: &[ColumnDef],
) -> Result<(Arc<str>, Schema)> {
    ensure!(
        cata.get(&name.lexeme).is_none(),
        Error::TableExists(name.clone())
    );

    let mut cols = Vec::with_capacity(defs.len());
    let mut seen = HashSet::with_capacity(defs.len());
    for def in defs {
        if !seen.insert(&def.name.lexeme) {
            bail!(Error::DuplicateColumn(def.name.clone()))
        }
        cols.push(Column::new(def.name.lexeme.clone(), def.ty));
    }

    Ok((name.lexeme.clone(), Schema::new_unchecked(cols)))
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

pub fn bind_select(cata: &Catalog, select: &ast::Select) -> Result<Plan> {
    let mut source = match &select.from {
        None => Plan::OneRow,
        Some(name) => {
            let table = cata
                .get(&name.lexeme)
                .ok_or_else(|| Error::TableNotFound(name.clone()))?;
            Plan::Scan {
                tid: table.name.clone(),
                schema: table.schema.clone(),
            }
        }
    };

    if let Some(were) = &select.were {
        let predicate = bind_expr(were, source.schema())?;
        ensure!(predicate.ty() == Type::Bool, Error::WhereTypeMustBeBool);
        source = Plan::Filter {
            child: Box::new(source),
            predicate,
        };
    }

    let mut projection = Vec::new();
    let mut schema = Schema::empty(); // output schema; built from the projection expressions
    for expr in &select.cols {
        match expr {
            Expr::Wildcard => {
                for (index, col) in source.schema().columns.iter().enumerate() {
                    projection.push(BoundExpr::Column { index, ty: col.ty });
                    schema.add_column(col.name.clone(), col.ty);
                }
            }
            _ => {
                let bound = bind_expr(expr, source.schema())?;
                let name = match expr {
                    Expr::Ident(ident) => ident.lexeme.clone(),
                    _ => "?column?".into(),
                };
                schema.add_column(name, bound.ty());
                projection.push(bound);
            }
        }
    }

    Ok(Plan::Project {
        child: Box::new(source),
        projection,
        schema: Arc::new(schema),
    })
}

pub fn bind_insert(cata: &Catalog, insert: &ast::Insert) -> Result<Plan> {
    let table = cata
        .get(&insert.into.lexeme)
        .ok_or_else(|| Error::TableNotFound(insert.into.clone()))?;

    ensure!(
        insert.vals.len() == table.schema.columns.len(),
        Error::ArityMismatch {
            table: insert.into.clone(),
            expected: table.schema.columns.len() as u16,
            found: insert.vals.len() as u16,
        }
    );

    let mut exprs = Vec::with_capacity(table.schema.columns.len());
    for (expr, col) in insert.vals.iter().zip(&table.schema.columns) {
        let bound = bind_expr(expr, &EMPTY_SCHEMA)?;
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

    Ok(Plan::Insert {
        tid: table.name.clone(),
        child: Box::new(Plan::Values {
            exprs: vec![exprs],
            schema: table.schema.clone(),
        }),
        schema: Arc::new(Schema::new_unchecked(vec![Column::new("count", Type::Int)])),
    })
}
