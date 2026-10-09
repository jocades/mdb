use crate::sql::ast::BinOp;
use crate::{plan::BoundExpr, value::Value};

#[derive(Debug)]
pub enum EvalError {
    Overflow,
    DivByZero,
}

pub fn eval(expr: &BoundExpr, env: &[Value]) -> Result<Value, EvalError> {
    match expr {
        // cloning is cheap since we are only cloning `Copy` values
        // or incrementing the `Arc<str>` refcount
        BoundExpr::Const(v) => Ok(v.clone()),
        BoundExpr::Column { index, .. } => Ok(env[*index].clone()),

        // short-circuit logical exprs
        BoundExpr::Bin {
            op: BinOp::And,
            lhs,
            rhs,
            ..
        } => match eval(lhs, env)? {
            v @ Value::Bool(false) => Ok(v),
            Value::Bool(true) => eval(rhs, env),
            _ => unreachable!(),
        },

        BoundExpr::Bin {
            op: BinOp::Or,
            lhs,
            rhs,
            ..
        } => match eval(lhs, env)? {
            v @ Value::Bool(true) => Ok(v),
            Value::Bool(false) => eval(rhs, env),
            _ => unreachable!(),
        },

        BoundExpr::Bin { op, lhs, rhs, .. } => {
            let lhs = eval(lhs, env)?;
            let rhs = eval(rhs, env)?;
            compute(*op, lhs, rhs)
        }
    }
}

fn compute(op: BinOp, lhs: Value, rhs: Value) -> Result<Value, EvalError> {
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

        (Gt, a, b) => Ok(Value::Bool(a > b)),
        (Ge, a, b) => Ok(Value::Bool(a >= b)),
        (Lt, a, b) => Ok(Value::Bool(a < b)),
        (Le, a, b) => Ok(Value::Bool(a <= b)),

        _ => unreachable!("binder guarantees operand types"),
    }
}
