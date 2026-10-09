use crate::sql::ast::BinOp;
use crate::value::{Type, Value};

#[derive(Debug)]
pub enum BoundExpr {
    Lit(Value),
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
}

#[derive(Debug)]
pub enum EvalError {
    Overflow,
    DivByZero,
}

impl BoundExpr {
    pub fn ty(&self) -> Type {
        match self {
            BoundExpr::Lit(v) => v.ty(),
            BoundExpr::Column { ty, .. } | BoundExpr::Bin { ty, .. } => *ty,
        }
    }

    pub fn eval(&self, scope: &[Value]) -> Result<Value, EvalError> {
        match self {
            BoundExpr::Lit(v) => Ok(v.clone()),
            BoundExpr::Column { index, .. } => Ok(scope[*index].clone()),
            BoundExpr::Bin { op, lhs, rhs, .. } => {
                let (lhs, rhs) = (lhs.eval(scope)?, rhs.eval(scope)?);
                use BinOp::*;
                match (op, lhs, rhs) {
                    (Add, Value::Int(a), Value::Int(b)) => {
                        a.checked_add(b).map(Value::Int).ok_or(EvalError::Overflow)
                    }
                    (Sub, Value::Int(a), Value::Int(b)) => {
                        a.checked_sub(b).map(Value::Int).ok_or(EvalError::Overflow)
                    }
                    (Mul, Value::Int(a), Value::Int(b)) => {
                        a.checked_mul(b).map(Value::Int).ok_or(EvalError::Overflow)
                    }
                    (Div, Value::Int(_), Value::Int(0)) => bail!(EvalError::DivByZero),
                    (Div, Value::Int(a), Value::Int(b)) => {
                        a.checked_div(b).map(Value::Int).ok_or(EvalError::Overflow)
                    }
                    (Eq, a, b) => Ok(Value::Bool(a == b)),
                    (Ne, a, b) => Ok(Value::Bool(a != b)),
                    (Gt, a, b) => todo!(),
                    (Ge, a, b) => todo!(),
                    (Lt, a, b) => todo!(),
                    (Le, a, b) => todo!(),
                    _ => unreachable!("binder guarantees operand types"),
                }
            }
        }
    }
}
