use std::fs;
use std::path::Path;
use std::process::Command;

pub fn get_tester_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

pub fn compile_schema(lang: &str, output_path: &Path) -> Result<(), String> {
    let tester_dir = get_tester_dir();
    let schema_path = tester_dir.join("schema.vol");

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
    }

    println!("  Compiling schema for {}...", lang);
    let compile_output = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "volexc",
            "--",
            "--input",
            schema_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
            "--lang",
            lang,
        ])
        .current_dir(tester_dir.parent().unwrap())
        .output()
        .map_err(|e| format!("failed to run compiler: {}", e))?;

    if !compile_output.status.success() {
        eprintln!("Compiler stderr:\n{}", String::from_utf8_lossy(&compile_output.stderr));
        return Err("schema compilation failed".to_string());
    }

    Ok(())
}
