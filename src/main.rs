mod checker;
mod codegen;
mod parser;
mod schema;

fn main() {
    let filename = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: lolserialize <file.lol>");
        std::process::exit(1);
    });

    let src = std::fs::read_to_string(&filename).unwrap_or_else(|e| {
        eprintln!("Error reading file '{}': {}", filename, e);
        std::process::exit(1);
    });

    let schema = match parser::parse(&src) {
        Ok(schema) => schema,
        Err(errs) => {
            parser::print_errors(&filename, &src, errs);
            std::process::exit(1);
        }
    };

    let errors = checker::check(&schema);
    if !errors.is_empty() {
        checker::print_errors(&filename, &src, errors);
        std::process::exit(1);
    }

    let code = codegen::generate(&schema);
    print!("{}", code);
}
