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
use repl::{Command, Input};
use storage::{DEFAULT_POOL_CAPACITY, Disk};

use clap::Parser;
use std::{
    io::{self, IsTerminal, Read},
    path::PathBuf,
};

use crate::storage::{FileDisk, MemDisk};

#[derive(Parser)]
struct Args {
    path: Option<PathBuf>,

    #[arg(long, default_value_t = DEFAULT_POOL_CAPACITY)]
    pool: usize,
}

fn main() {
    let args = Args::parse();
    match args.path {
        None => run(Database::with_disk(MemDisk::default(), args.pool).unwrap()),
        Some(path) => run(Database::with_disk(FileDisk::open(path).unwrap(), args.pool).unwrap()),
    }
    // let mut args = std::env::args();
    // match args.len() {
    //     1 => run(Database::memory()),
    //     2 => {
    //         let path = args.nth(1).unwrap();
    //         run(Database::open(&path).unwrap())
    //     }
    //     _ => {
    //         eprintln!("usage: mbd [PATH]");
    //         std::process::exit(1);
    //     }
    // };
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

    for input in repl::input() {
        match input {
            Input::Code(source) => execute(&mut db, &source),
            Input::Command(Ok(cmd)) => run_command(&mut db, cmd),
            Input::Command(Err(what)) => println!("error: unknown command `{what}`"),
        }
    }
}

fn execute(db: &mut Database<impl Disk>, sql: &str) {
    match db.execute_batch(&sql) {
        Err(err) => report(err, &sql),
        Ok(results) => results.iter().for_each(|res| println!("{res}")),
    };
}

fn run_command(db: &mut Database<impl Disk>, cmd: Command) {
    match cmd {
        Command::Catalog => {
            dbg!(&db.cata);
        }
    }
}

fn report(err: database::Error, src: &str) {
    use database::Error;
    match err {
        Error::Parse(e) => report_parse_error(e, src),
        Error::Bind(e) => match e {
            plan::binder::Error::TableExists(ident) => {
                simple(format!("table `{ident}` already exists"), ident.span, src)
            }
            plan::binder::Error::DuplicateColumn(ident) => {
                simple(format!("duplicate column `{ident}`"), ident.span, src)
            }
            plan::binder::Error::TableNotFound(ident) => {
                simple(format!("no such table `{ident}`"), ident.span, src)
            }
            plan::binder::Error::ColumnNotFound(ident) => {
                simple(format!("no such column `{ident}`"), ident.span, src)
            }
            plan::binder::Error::InvalidOperands { op, lty, rty } => {
                println!("error: invalid operator `{op:?}` for `{lty:?}` and `{rty:?}`");
            }
            plan::binder::Error::ArityMismatch {
                table,
                expected,
                found,
            } => {
                println!(
                    "error: arity mismatch, table `{table}` has {expected} columns but found {found} expressions"
                )
            }
            plan::binder::Error::ColumnTypeMismatch {
                column,
                expected,
                found,
            } => {
                println!(
                    "error: column type mismatch, expected type `{expected:?}` for column {column} but found `{found:?}"
                )
            }
            plan::binder::Error::WhereTypeMustBeBool => {
                println!("error: where type must be boolean");
            }
        },
        Error::Io(_) | Error::Catalog(_) | Error::Exec(_) => println!("error: {err:?}"),
    }
}

fn report_parse_error(err: sql::parser::Error, src: &str) {
    use ariadne::{Label, Report, ReportKind, Source};
    use sql::{lexer, parser::Error};
    use std::fmt::Write;
    match err {
        Error::Expected {
            expected,
            found,
            span,
        } => {
            let mut m = "expected ".to_string();
            if expected.len() > 1 {
                m.push_str("one of ");
                for (i, exp) in expected.iter().enumerate() {
                    _ = write!(m, "`{exp:?}`");
                    if i != expected.len() - 1 {
                        m.push_str(", ");
                    }
                }
            } else {
                _ = write!(m, "{:?}", expected[0]);
            }
            _ = write!(m, " but found `{found:?}`");
            simple(m, span, src);
        }
        Error::ExpectedExpression(span) => simple("expected expression", span, src),
        Error::ExpectedType(span) => simple("expected type", span, src),
        Error::UnknwonType(span) => simple(
            format!("unknown type `{}`", &src[span.start..span.end]),
            span,
            src,
        ),
        Error::UnexpectedEof(span) => simple(format!("unexpected end of file"), span, src),
        Error::Lex(error) => match error {
            lexer::Error::UnexpectedCharacter { ch, at } => {
                simple(
                    format!("unexpected character `{ch}`"),
                    (at..at + 1).into(),
                    src,
                );
            }
            lexer::Error::UnterminatedString { span } => {
                Report::build(ReportKind::Error, span)
                    .with_message("unterminated string")
                    .with_label(Label::new(span).with_message("missing closing quote"))
                    .finish()
                    .print(Source::from(src))
                    .unwrap();
            }
        },
        _ => println!("error: {err:?}"),
    }
}

fn simple(message: impl AsRef<str>, span: sql::lexer::Span, src: &str) {
    use ariadne::{Label, Report, ReportKind, Source};
    Report::build(ReportKind::Error, span)
        .with_message(message.as_ref())
        .with_label(Label::new(span).with_message("here"))
        .finish()
        .print(Source::from(src))
        .unwrap();
}
