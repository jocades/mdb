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

use database::{Database, QueryResult};
use repl::Repl;
use storage::Disk;

use std::io::{self, IsTerminal, Read};

fn main() {
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

fn run<D: Disk>(mut db: Database<D>) {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        let mut buf = String::new();
        if stdin.lock().read_to_string(&mut buf).is_ok() {
            execute(&mut db, &buf);
        }
        return;
    }

    for source in Repl::new() {
        let trimmed = source.trim();
        if trimmed.starts_with("/") {
            run_command(&mut db, &trimmed[1..trimmed.len() - 1]);
            continue;
        }
        execute(&mut db, &source);
    }
}

fn execute(db: &mut Database<impl Disk>, sql: &str) {
    let results = match db.execute_batch(&sql) {
        Ok(res) => res,
        Err(err) => {
            report(err, &sql);
            return;
        }
    };

    for result in results {
        match result {
            QueryResult::None => {}
            QueryResult::Affected(n) => {
                println!("{n} row(s) affected");
            }
            QueryResult::Rows { rows, schema } => {
                if rows.len() != 0 {
                    let mut b = tabled::builder::Builder::new();
                    b.push_record(schema.columns.iter().map(|col| col.name.as_ref()));
                    for row in &rows {
                        b.push_record(row.iter().map(ToString::to_string));
                    }
                    println!("{}", b.build());
                }
                println!("({} rows)", rows.len());
            }
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
        // TableExists(ident) => {
        //     Report::build(ReportKind::Error, ident.span)
        //         .with_message(format!("table `{ident}` already exists"))
        //         .with_label(Label::new(ident.span).with_message("here"))
        //         .finish()
        //         .print(Source::from(src))
        //         .unwrap();
        // }
        _ => println!("error: {err:?}"),
    }
}
