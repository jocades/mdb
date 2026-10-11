use std::collections::HashSet;

use crate::catalog::{Catalog, Column, Schema};
use crate::sql::ast::{ColumnDef, Ident};

use super::{Error, Result, Util};

pub fn create_table(cata: &Catalog, name: Ident, defs: Vec<ColumnDef>) -> Result<Util> {
    ensure!(cata.get(&name.lexeme).is_none(), Error::TableExists(name));

    let mut cols = Vec::with_capacity(defs.len());
    let mut seen = HashSet::with_capacity(defs.len());

    for def in &defs {
        if !seen.insert(&def.name.lexeme) {
            bail!(Error::DuplicateColumn(def.name.clone()))
        }
        cols.push(Column::new(def.name.lexeme.clone(), def.ty));
    }

    Ok(Util::CreateTable {
        name: name.lexeme,
        schema: Schema::new_unchecked(cols),
    })
}

pub fn drop_table(cata: &Catalog, name: Ident) -> Result<Util> {
    ensure!(cata.get(&name.lexeme).is_some(), Error::TableNotFound(name));
    Ok(Util::DropTable { name: name.lexeme })
}
