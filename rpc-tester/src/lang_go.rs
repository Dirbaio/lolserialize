use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use crate::common::{compile_schema, get_tester_dir};

pub fn start_server() -> Result<(Child, String), String> {
    let tester_dir = get_tester_dir();
    let go_harness_dir = tester_dir.join("harness/go");
    let generated_path = go_harness_dir.join("gen/generated.go");

    compile_schema("go", &generated_path)?;

    // Fix package name
    let code = std::fs::read_to_string(&generated_path).map_err(|e| format!("read: {}", e))?;
    let code = code.replace("package main", "package gen");
    std::fs::write(&generated_path, code).map_err(|e| format!("write: {}", e))?;

    println!("  Building Go server...");
    let build_output = Command::new("go")
        .args(["build", "-o", "server", "./cmd/server"])
        .current_dir(&go_harness_dir)
        .output()
        .map_err(|e| format!("failed to build go server: {}", e))?;
    if !build_output.status.success() {
        eprintln!("Build stderr:\n{}", String::from_utf8_lossy(&build_output.stderr));
        return Err("go build server failed".to_string());
    }

    println!("  Starting Go server...");
    let mut server = Command::new("./server")
        .current_dir(&go_harness_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start go server: {}", e))?;

    let addr = {
        let stdout = server.stdout.take().ok_or("failed to get server stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut addr = String::new();
        reader
            .read_line(&mut addr)
            .map_err(|e| format!("failed to read server address: {}", e))?;
        addr.trim().to_string()
    };
    println!("  Go server listening on {}", addr);

    Ok((server, addr))
}

pub fn run_client(addr: &str) -> Result<(), String> {
    let tester_dir = get_tester_dir();
    let go_harness_dir = tester_dir.join("harness/go");
    let generated_path = go_harness_dir.join("gen/generated.go");

    compile_schema("go", &generated_path)?;

    // Fix package name
    let code = std::fs::read_to_string(&generated_path).map_err(|e| format!("read: {}", e))?;
    let code = code.replace("package main", "package gen");
    std::fs::write(&generated_path, code).map_err(|e| format!("write: {}", e))?;

    println!("  Running Go client tests...");
    let test_output = Command::new("go")
        .args(["test", "-v", "./cmd/client"])
        .current_dir(&go_harness_dir)
        .env("SERVER_ADDR", addr)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("go test: {}", e))?;

    println!("{}", String::from_utf8_lossy(&test_output.stdout));
    if !test_output.stderr.is_empty() {
        eprintln!("{}", String::from_utf8_lossy(&test_output.stderr));
    }

    if !test_output.status.success() {
        return Err("go client test failed".to_string());
    }

    Ok(())
}
