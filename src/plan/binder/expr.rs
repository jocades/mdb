use crate::catalog::Schema;
use crate::executor::functions::{self, Func, ScalarFn};
use crate::sql::ast::{BinOp, Expr, Ident, Lit};
use crate::value::{Type, Value};

use super::{Binder, Error, Result};

#[derive(Debug)]
pub enum BoundExpr {
    Const(Value),
    Column {
        index: usize,
        ty: Type,
    },
    Bin {
        op: BinOp,
        lhs: Box<BoundExpr>,
        rhs: Box<BoundExpr>,
        ty: Type,
    },
    // only support builtins for now
    Call {
        func: &'static ScalarFn,
        args: Vec<BoundExpr>,
    },
}

impl BoundExpr {
    pub fn ty(&self) -> Type {
        match self {
            BoundExpr::Const(v) => v.ty(),
            BoundExpr::Column { ty, .. }
            | BoundExpr::Bin { ty, .. }
            | BoundExpr::Call {
                func: ScalarFn { ret: ty, .. },
                ..
            } => *ty,
        }
    }
}

impl Binder<'_> {
    pub fn expr(&self, expr: &Expr, scope: &Schema) -> Result<BoundExpr> {
        match expr {
            Expr::Lit(lit) => Ok(BoundExpr::Const(match lit {
                Lit::Int(n) => Value::Int(*n),
                Lit::Bool(b) => Value::Bool(*b),
                Lit::String(s) => Value::String(s.clone()),
            })),

            Expr::Ident(name) => {
                let index = scope
                    .index_of(&name.lexeme)
                    .ok_or_else(|| Error::ColumnNotFound(name.clone()))?;

                Ok(BoundExpr::Column {
                    index,
                    ty: scope.columns[index].ty,
                })
            }

            Expr::Bin(lhs, op, rhs) => self.bin(*op, lhs, rhs, scope),

            Expr::Call(name, args) => self.call(name, args, scope),
        }
    }

    fn bin(&self, op: BinOp, lhs: &Expr, rhs: &Expr, scope: &Schema) -> Result<BoundExpr> {
        let (lhs, rhs) = (self.expr(lhs, scope)?, self.expr(rhs, scope)?);
        let (lty, rty) = (lhs.ty(), rhs.ty());

        use BinOp::*;
        let ty = match (op, lty, rty) {
            (Add | Sub | Mul | Div, Type::Int, Type::Int) => Type::Int,
            (And | Or, Type::Bool, Type::Bool) => Type::Bool,
            (Eq | Ne | Gt | Ge | Lt | Le, a, b) if a == b => Type::Bool,
            _ => bail!(Error::InvalidOperands { op, lty, rty }),
        };

        Ok(BoundExpr::Bin {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            ty,
        })
    }

    fn call(&self, name: &Ident, args: &[Expr], scope: &Schema) -> Result<BoundExpr> {
        let Some(Func::Scalar(func)) = functions::lookup(&name.lexeme) else {
            bail!(Error::FunctionNotFound(name.clone()))
        };
        ensure!(
            args.len() == func.args.len(),
            Error::FunctionArity {
                name: name.clone(),
                expected: func.args.len(),
                found: args.len()
            }
        );
        let mut bound = Vec::with_capacity(args.len());
        for (arg, &want) in args.iter().zip(func.args) {
            let b = self.expr(arg, scope)?;
            ensure!(
                b.ty() == want,
                Error::ArgumentType {
                    name: name.clone(),
                    expected: want,
                    found: b.ty(),
                }
            );
            bound.push(b);
        }

        Ok(BoundExpr::Call { func, args: bound })
    }
}
