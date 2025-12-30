mod common;
mod lang_go;
mod lang_rust;
mod lang_typescript;

use std::io::{self, Write};
use std::process::Child;

use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(name = "rpc-tester")]
#[command(about = "RPC test runner for volex")]
struct Cli {
    /// Client language (omit with --all to run all combinations)
    #[arg(long, value_enum, required_unless_present = "all")]
    client: Option<Language>,

    /// Server language (omit with --all to run all combinations)
    #[arg(long, value_enum, required_unless_present = "all")]
    server: Option<Language>,

    /// Run all client+server combinations
    #[arg(long)]
    all: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum, Debug)]
enum Language {
    Go,
    Rust,
    Typescript,
}

impl Language {
    fn name(&self) -> &'static str {
        match self {
            Language::Go => "Go",
            Language::Rust => "Rust",
            Language::Typescript => "TypeScript",
        }
    }
}

fn start_server(lang: Language) -> Result<(Child, String), String> {
    match lang {
        Language::Go => lang_go::start_server(),
        Language::Rust => lang_rust::start_server(),
        Language::Typescript => Err("TypeScript server not yet implemented".to_string()),
    }
}

fn run_client(lang: Language, addr: &str) -> Result<(), String> {
    match lang {
        Language::Go => lang_go::run_client(addr),
        Language::Rust => lang_rust::run_client(addr),
        Language::Typescript => lang_typescript::run_client(addr),
    }
}

fn run_test(client_lang: Language, server_lang: Language) -> Result<(), String> {
    println!();
    println!("=== {} client + {} server ===", client_lang.name(), server_lang.name());

    // Start the server
    let (mut server, addr) = start_server(server_lang)?;

    // Run the client tests
    let result = run_client(client_lang, &addr);

    // Kill server
    let _ = server.kill();
    let _ = server.wait();

    result
}

fn main() {
    let cli = Cli::parse();

    let combinations: Vec<(Language, Language)> = if cli.all {
        let clients = [Language::Go, Language::Rust, Language::Typescript];
        let servers = [Language::Go, Language::Rust];

        clients
            .iter()
            .flat_map(|&c| servers.iter().map(move |&s| (c, s)))
            .collect()
    } else {
        vec![(cli.client.unwrap(), cli.server.unwrap())]
    };

    let mut passed = 0;
    let mut failed = 0;
    let mut failed_combos = Vec::new();

    for (client_lang, server_lang) in &combinations {
        match run_test(*client_lang, *server_lang) {
            Ok(()) => {
                passed += 1;
            }
            Err(e) => {
                eprintln!(
                    "❌ {} client + {} server: {}",
                    client_lang.name(),
                    server_lang.name(),
                    e
                );
                failed += 1;
                failed_combos.push((*client_lang, *server_lang));
            }
        }
    }

    println!();
    if failed == 0 {
        println!("✅ All {} test(s) passed!", passed);
        io::stdout().flush().unwrap();
    } else {
        eprintln!("❌ {}/{} test(s) failed:", failed, passed + failed);
        for (c, s) in &failed_combos {
            eprintln!("  - {} client + {} server", c.name(), s.name());
        }
        io::stderr().flush().unwrap();
        std::process::exit(1);
    }
}
