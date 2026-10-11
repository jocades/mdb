use super::EvalError;
use crate::value::{Type, Value};

pub enum Func {
    Scalar(&'static ScalarFn),
}

#[derive(Debug)]
pub struct ScalarFn {
    pub name: &'static str,
    pub args: &'static [Type],
    pub ret: Type,
    pub eval: fn(&[Value]) -> Result<Value, EvalError>,
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
