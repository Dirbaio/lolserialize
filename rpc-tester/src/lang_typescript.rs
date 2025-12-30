use std::process::{Command, Stdio};

use crate::common::{compile_schema, get_tester_dir};

pub fn run_client(addr: &str) -> Result<(), String> {
    let tester_dir = get_tester_dir();
    let ts_harness_dir = tester_dir.join("harness/ts");

    // Compile schema directly to harness
    compile_schema("typescript", &ts_harness_dir.join("generated.ts"))?;

    // Install dependencies if needed
    if !ts_harness_dir.join("node_modules").exists() {
        println!("  Installing TypeScript dependencies...");
        let install_output = Command::new("npm")
            .args(["install"])
            .current_dir(&ts_harness_dir)
            .output()
            .map_err(|e| format!("npm install: {}", e))?;

        if !install_output.status.success() {
            eprintln!(
                "npm install stderr:\n{}",
                String::from_utf8_lossy(&install_output.stderr)
            );
            return Err("npm install failed".to_string());
        }
    }

    println!("  Running TypeScript client tests...");
    let test_output = Command::new("npx")
        .args(["tsx", "client.ts"])
        .current_dir(&ts_harness_dir)
        .env("SERVER_ADDR", addr)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("npx tsx: {}", e))?;

    println!("{}", String::from_utf8_lossy(&test_output.stdout));
    if !test_output.stderr.is_empty() {
        eprintln!("{}", String::from_utf8_lossy(&test_output.stderr));
    }

    if !test_output.status.success() {
        return Err("ts client test failed".to_string());
    }

    Ok(())
}
