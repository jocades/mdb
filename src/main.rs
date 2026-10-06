#[macro_use]
mod macros;
mod bytes;
mod catalog;
mod database;
mod heap;
mod repl;
mod sql;
mod storage;
mod value;

use database::Database;
use repl::Repl;
use storage::Disk;

fn run<D: Disk>(mut db: Database<D>) {
    dbg!(&db.catalog);

    for source in Repl::new() {
        if let Err(e) = db.execute(&source) {
            println!("error: {e:?}");
        }
    }
}

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
