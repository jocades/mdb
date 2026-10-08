#[macro_use]
mod macros;
mod bytes;
mod catalog;
mod database;
mod exec;
mod heap;
mod plan;
mod repl;
mod sql;
mod storage;
mod value;

use std::fs::File;

use database::Database;
use repl::Repl;
use storage::Disk;

use crate::storage::{FileDisk, MemDisk};

fn main() {
    // debug_file();
    let mut args = std::env::args();
    match args.len() {
        1 => run(Database::memory()),
        2 => {
            let path = args.nth(1).unwrap();
            run(Database::open(&path).unwrap())
        }
        _ => {
            eprintln!("usage: mbd [PATH]");
            std::process::exit(1);
        }
    };
}

fn debug_file() {
    let mut db = Database::with_disk(FileDisk::open("./bak/db").unwrap(), 2).unwrap();
}

fn debug_mem() {
    let mut db = Database::with_disk(MemDisk::default(), 2).unwrap();
    db.execute("create table t (name text, value int);")
        .unwrap();
    db.execute("insert into t values ('foo', 42);").unwrap();
    db.execute("select value, name from t;").unwrap();
}

fn run<D: Disk>(mut db: Database<D>) {
    dbg!(&db.cata);

    for source in Repl::new() {
        let trimmed = source.trim();
        if trimmed.starts_with("/") {
            run_command(&mut db, &trimmed[1..trimmed.len() - 1]);
            continue;
        }

        if let Err(e) = db.execute(&source) {
            report(e, &source);
        }
    }
}

fn run_command(db: &mut Database<impl Disk>, cmd: &str) {
    match cmd {
        "catalog" => {
            dbg!(&db.cata);
        }
        _ => eprintln!("error: unknown command `{cmd}`"),
    }
}

fn report(err: database::Error, src: &str) {
    use ariadne::{Label, Report, ReportKind, Source};
    use database::Error::*;
    match err {
        TableExists(ident) => {
            Report::build(ReportKind::Error, ident.span)
                .with_message(format!("table `{ident}` already exists"))
                .with_label(Label::new(ident.span).with_message("here"))
                .finish()
                .print(Source::from(src))
                .unwrap();
        }
        _ => println!("error: {err:?}"),
        // Io(e) => println!("error: {e}"),
        // Parse(e) => println!("error: {e:?}"),
        // TableNotFound(ident) => todo!(),
        // ColumnNotFound { column, table } => todo!(),
        // ArityMismatch {
        //     table,
        //     expected,
        //     found,
        // } => todo!(),
        // TypeMismatch { expected, found } => todo!(),
    }
}
