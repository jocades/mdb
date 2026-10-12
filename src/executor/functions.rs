use super::EvalError;
use crate::value::{Type, Value};

pub enum Func {
    Scalar(&'static ScalarFn),
    Aggregate(&'static AggregateFn),
}

#[derive(Debug)]
pub struct ScalarFn {
    pub name: &'static str,
    pub args: &'static [Type],
    pub ret: Type,
    pub eval: fn(&[Value]) -> Result<Value, EvalError>,
}

pub enum AggArg {
    All,                  // count(*)
    One(&'static [Type]), // sum(int)
}

pub struct AggregateFn {
    pub name: &'static str,
    pub arg: AggArg,
    pub ret: fn(arg: Option<Type>) -> Type, // min/max return their arg type
    pub init: fn() -> AggState,
    pub update: fn(&mut AggState, Option<&Value>) -> Result<(), EvalError>,
    pub finish: fn(AggState) -> Value,
}

pub enum AggState {
    Count(i64),
    Sum(Option<i64>),
    Best(Option<Value>),
}

#[rustfmt::skip]
static SCALARS: &[ScalarFn] = &[
    ScalarFn { name: "upper", args: &[Type::Text], ret: Type::Text, eval: upper }
];

fn upper(args: &[Value]) -> Result<Value, EvalError> {
    let Value::String(s) = &args[0] else { unreachable!() };
    Ok(Value::String(s.to_uppercase().into()))
}

pub fn lookup(name: &str) -> Option<Func> {
    SCALARS
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(name))
        .map(Func::Scalar)
}
