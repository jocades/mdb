use super::{Type, Value};
use crate::bytes::{BufRead, BufWrite};
use crate::catalog::Schema;

pub fn encode(schema: &Schema, tuple: &[Value]) -> Vec<u8> {
    assert_eq!(schema.columns.len(), tuple.len());

    let mut buf = Vec::new();
    for (col, val) in schema.columns.iter().zip(tuple) {
        match (col.ty, val) {
            (Type::Int, Value::Int(n)) => buf.write_i64(*n),
            (Type::Bool, Value::Bool(b)) => buf.write_u8(*b as u8),
            (Type::Text, Value::String(s)) => {
                assert!(s.len() <= u16::MAX as usize);
                buf.write_u16(s.len() as u16);
                buf.write_slice(s.as_bytes());
            }
            _ => unreachable!("type check failed"),
        }
    }
    buf
}

pub fn decode(schema: &Schema, mut buf: &[u8]) -> Vec<Value> {
    let mut tuple = Vec::with_capacity(schema.columns.len());
    for col in &schema.columns {
        tuple.push(match col.ty {
            Type::Int => Value::Int(buf.read_i64()),
            Type::Bool => Value::Bool(buf.read_u8() != 0),
            Type::Text => {
                let len = buf.read_u16() as usize;
                let s = buf.read_str(len);
                Value::String(s.into())
            }
        })
    }
    tuple
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Column;

    #[test]
    fn it_works() {
        let schema = Schema::new_unchecked(vec![
            Column::new("integer", Type::Int),
            Column::new("boolean", Type::Bool),
            Column::new("string", Type::Text),
        ]);

        let tuple = vec![
            Value::Int(42),
            Value::Bool(true),
            Value::String("alpha".into()),
        ];

        assert_eq!(decode(&schema, &encode(&schema, &tuple)), tuple);
    }
}
