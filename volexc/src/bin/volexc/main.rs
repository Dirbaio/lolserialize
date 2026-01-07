use std::io::{Read, Write};

use ariadne::{Color, Label, Report, ReportKind, Source};
use clap::Parser;
use volexc::{CompileError, Language, codegen_go, codegen_rust, codegen_typescript};

#[derive(Parser)]
#[command(name = "volex")]
#[command(about = "Volex schema compiler", long_about = None)]
struct Cli {
    /// Input schema file (use '-' for stdin)
    #[arg(short, long, value_name = "FILE")]
    input: String,

    /// Output file (use '-' for stdout)
    #[arg(short, long, value_name = "FILE")]
    output: String,

    /// Target language
    #[arg(long, default_value = "rust")]
    lang: Language,

    /// Package name for Go output
    #[arg(long, default_value = "main")]
    go_package: String,

    /// Serde derive mode for Rust output
    #[arg(long, value_enum, default_value = "never")]
    serde: codegen_rust::SerdeMode,
}

fn main() {
    let cli = Cli::parse();

    // Read input
    let src = if cli.input == "-" {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer).unwrap_or_else(|e| {
            eprintln!("Error reading from stdin: {}", e);
            std::process::exit(1);
        });
        buffer
    } else {
        std::fs::read_to_string(&cli.input).unwrap_or_else(|e| {
            eprintln!("Error reading file '{}': {}", cli.input, e);
            std::process::exit(1);
        })
    };

    let input_name = if cli.input == "-" { "<stdin>" } else { &cli.input };

    let (schema, errs) = volexc::compile(&src, cli.lang);
    if !errs.is_empty() {
        print_errors(input_name, &src, errs);
        std::process::exit(1);
    };
    let schema = schema.unwrap();

    let code = match cli.lang {
        Language::Rust => codegen_rust::generate(&schema, cli.serde),
        Language::Go => codegen_go::generate(&schema, &cli.go_package),
        Language::Typescript => codegen_typescript::generate(&schema),
    };

    // Write output
    if cli.output == "-" {
        print!("{}", code);
        std::io::stdout().flush().unwrap();
    } else {
        std::fs::write(&cli.output, code).unwrap_or_else(|e| {
            eprintln!("Error writing file '{}': {}", cli.output, e);
            std::process::exit(1);
        });
    }
}

fn print_errors(filename: &str, src: &str, errs: Vec<CompileError>) {
    for err in errs {
        let err_span = err.span.start..err.span.end;
        let mut report = Report::build(ReportKind::Error, (filename, err_span.clone())).with_message(&err.message);

        if !err.labels.is_empty() {
            // Add labels
            for (span, label_msg, color) in err.labels {
                report = report.with_label(
                    Label::new((filename, span.start..span.end))
                        .with_message(label_msg)
                        .with_color(color),
                );
            }
        } else {
            // Add a simple label for errors without labels
            report = report.with_label(
                Label::new((filename, err_span))
                    .with_message("Error here")
                    .with_color(Color::Red),
            );
        }

        // Add notes
        for note in err.notes {
            report = report.with_note(note);
        }

        report.finish().print((filename, Source::from(src))).unwrap();
    }
}
