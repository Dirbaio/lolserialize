use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use crate::common::{compile_schema, get_tester_dir};

fn build_binary(bin_name: &str) -> Result<(), String> {
    let tester_dir = get_tester_dir();
    println!("  Building Rust {}...", bin_name);
    let build_output = Command::new("cargo")
        .args(["build", "-p", "rust-harness", "--bin", bin_name])
        .current_dir(tester_dir.parent().unwrap())
        .output()
        .map_err(|e| format!("failed to build rust {}: {}", bin_name, e))?;

    if !build_output.status.success() {
        eprintln!("Build stdout:\n{}", String::from_utf8_lossy(&build_output.stdout));
        eprintln!("Build stderr:\n{}", String::from_utf8_lossy(&build_output.stderr));
        return Err(format!("rust {} build failed", bin_name));
    }

    Ok(())
}

pub fn start_server() -> Result<(Child, String), String> {
    let tester_dir = get_tester_dir();
    let generated_path = tester_dir.join("harness/rust/src/generated.rs");

    compile_schema("rust", &generated_path)?;
    build_binary("server")?;

    println!("  Starting Rust server...");
    let server_bin = tester_dir.parent().unwrap().join("target/debug/server");
    let mut server = Command::new(&server_bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("failed to start rust server: {}", e))?;

    let addr = {
        let stdout = server.stdout.take().ok_or("failed to get server stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut addr = String::new();
        reader
            .read_line(&mut addr)
            .map_err(|e| format!("failed to read server address: {}", e))?;
        addr.trim().to_string()
    };
    println!("  Rust server listening on {}", addr);

    Ok((server, addr))
}

pub fn run_client(addr: &str) -> Result<(), String> {
    let tester_dir = get_tester_dir();
    let generated_path = tester_dir.join("harness/rust/src/generated.rs");

    compile_schema("rust", &generated_path)?;
    build_binary("client")?;

    let client_bin = tester_dir.parent().unwrap().join("target/debug/client");

    println!("  Running Rust client tests...");
    let test_output = Command::new(&client_bin)
        .env("SERVER_ADDR", addr)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("rust client: {}", e))?;

    println!("{}", String::from_utf8_lossy(&test_output.stdout));
    if !test_output.stderr.is_empty() {
        eprintln!("{}", String::from_utf8_lossy(&test_output.stderr));
    }

    if !test_output.status.success() {
        return Err("rust client test failed".to_string());
    }

    Ok(())
}
