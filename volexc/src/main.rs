mod checker;
mod codegen_go;
mod codegen_rust;
mod codegen_typescript;
mod parser;
mod schema;

use std::io::{Read, Write};

use clap::Parser;

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
    #[arg(long, value_enum, default_value = "rust")]
    lang: Language,

    /// Package name for Go output
    #[arg(long, default_value = "main")]
    go_package: String,
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Language {
    Rust,
    Go,
    Typescript,
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

    let schema = match parser::parse(&src) {
        Ok(schema) => schema,
        Err(errs) => {
            parser::print_errors(input_name, &src, errs);
            std::process::exit(1);
        }
    };

    let errors = checker::check(&schema);
    if !errors.is_empty() {
        checker::print_errors(input_name, &src, errors);
        std::process::exit(1);
    }

    let code = match cli.lang {
        Language::Rust => codegen_rust::generate(&schema),
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
