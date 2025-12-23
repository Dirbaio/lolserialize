mod checker;
mod codegen;
mod codegen_go;
mod parser;
mod schema;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: lolserialize [--lang <rust|go>] [--package <name>] <file.lol>");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --lang <rust|go>    Target language (default: rust)");
        eprintln!("  --package <name>    Package name for Go output (default: main)");
        std::process::exit(1);
    }

    let mut lang = "rust";
    let mut package = "main";
    let mut filename = None;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--lang" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --lang requires an argument");
                    std::process::exit(1);
                }
                lang = &args[i + 1];
                if lang != "rust" && lang != "go" {
                    eprintln!("Error: language must be 'rust' or 'go'");
                    std::process::exit(1);
                }
                i += 2;
            }
            "--package" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --package requires an argument");
                    std::process::exit(1);
                }
                package = &args[i + 1];
                i += 2;
            }
            arg if !arg.starts_with("--") => {
                filename = Some(arg);
                i += 1;
            }
            _ => {
                eprintln!("Error: unknown option '{}'", args[i]);
                std::process::exit(1);
            }
        }
    }

    let filename = filename.unwrap_or_else(|| {
        eprintln!("Error: no input file specified");
        std::process::exit(1);
    });

    let src = std::fs::read_to_string(filename).unwrap_or_else(|e| {
        eprintln!("Error reading file '{}': {}", filename, e);
        std::process::exit(1);
    });

    let schema = match parser::parse(&src) {
        Ok(schema) => schema,
        Err(errs) => {
            parser::print_errors(filename, &src, errs);
            std::process::exit(1);
        }
    };

    let errors = checker::check(&schema);
    if !errors.is_empty() {
        checker::print_errors(filename, &src, errors);
        std::process::exit(1);
    }

    let code = match lang {
        "rust" => codegen::generate(&schema),
        "go" => codegen_go::generate(&schema, package),
        _ => unreachable!(),
    };

    print!("{}", code);
}
