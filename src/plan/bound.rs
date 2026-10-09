use crate::sql::ast::BinOp;
use crate::value::{Type, Value};

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
}

impl BoundExpr {
    pub fn ty(&self) -> Type {
        match self {
            BoundExpr::Const(v) => v.ty(),
            BoundExpr::Column { ty, .. } | BoundExpr::Bin { ty, .. } => *ty,
        }
    }
}
