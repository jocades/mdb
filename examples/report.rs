use ariadne::{Color, Label, Report, ReportKind, Source};

fn main() {
    let source = r#"
select one, two
from foo
where id = 1;
"#;

    let report = Report::build(ReportKind::Error, 10..15)
        .with_message("Column 'one' does not exist in table 'foo'")
        .with_label(
            Label::new(8..11)
                .with_message("column `one` does not exist")
                .with_color(Color::Red),
        )
        .with_label(
            Label::new(21..24)
                .with_message("in table `foo`")
                .with_color(Color::Green),
        )
        .with_label(
            Label::new(source.len() - 1..source.len() - 1)
                .with_message("at end")
                .with_color(Color::BrightGreen),
        )
        .finish();
    report.print(Source::from(source)).unwrap();
}
