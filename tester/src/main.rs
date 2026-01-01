mod lang_go;
mod lang_rust;
mod lang_typescript;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command};

use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(name = "test-runner")]
#[command(about = "Cross-implementation test runner for volex")]
struct Cli {
    /// Test suite name (e.g., "primitives", "structs"). If not specified, runs all suites.
    suite: Option<String>,

    /// Implementation language to test
    #[arg(long, value_enum)]
    lang: Language,

    /// Regenerate hex values from JSON (fixes test data)
    #[arg(long)]
    regen_hex: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Language {
    Rust,
    Go,
    Typescript,
}

impl Language {
    fn as_str(&self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Go => "go",
            Language::Typescript => "typescript",
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_empty_vec(v: &Vec<String>) -> bool {
    v.is_empty()
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct TestCase {
    r#type: String,
    description: String,
    hex: String,
    json: serde_json::Value,
    #[serde(default, skip_serializing_if = "is_false")]
    nondeterministic_encode: bool,
    #[serde(default, skip_serializing_if = "is_empty_vec")]
    skip_languages: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "cmd")]
enum Request {
    #[serde(rename = "encode")]
    Encode { r#type: String, json: serde_json::Value },
    #[serde(rename = "decode")]
    Decode { r#type: String, hex: String },
}

#[derive(Debug, serde::Deserialize)]
struct Response {
    ok: bool,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
}

struct TestBinary {
    process: Child,
}

impl TestBinary {
    fn send_request(&mut self, req: &Request) -> Result<Response, String> {
        let stdin = self.process.stdin.as_mut().ok_or("failed to get stdin")?;
        let stdout = self.process.stdout.as_mut().ok_or("failed to get stdout")?;

        let req_json = serde_json::to_string(req).map_err(|e| format!("serialize error: {}", e))?;
        writeln!(stdin, "{}", req_json).map_err(|e| format!("write error: {}", e))?;
        stdin.flush().map_err(|e| format!("flush error: {}", e))?;

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| format!("read error: {}", e))?;

        let resp: Response = serde_json::from_str(&line).map_err(|e| format!("deserialize error: {}", e))?;

        Ok(resp)
    }

    fn encode(&mut self, type_name: &str, json: serde_json::Value) -> Result<String, String> {
        let req = Request::Encode {
            r#type: type_name.to_string(),
            json,
        };
        let resp = self.send_request(&req)?;

        if resp.ok {
            resp.result
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .ok_or_else(|| "missing or invalid result".to_string())
        } else {
            Err(resp.error.unwrap_or_else(|| "unknown error".to_string()))
        }
    }

    fn decode(&mut self, type_name: &str, hex: &str) -> Result<serde_json::Value, String> {
        let req = Request::Decode {
            r#type: type_name.to_string(),
            hex: hex.to_string(),
        };
        let resp = self.send_request(&req)?;

        if resp.ok {
            resp.result.ok_or_else(|| "missing result".to_string())
        } else {
            Err(resp.error.unwrap_or_else(|| "unknown error".to_string()))
        }
    }
}

impl Drop for TestBinary {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn json_eq_with_float_tolerance(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    const FLOAT_TOLERANCE: f64 = 1e-6;
    const RELATIVE_TOLERANCE: f64 = 1e-6;

    match (a, b) {
        (serde_json::Value::Number(a_num), serde_json::Value::Number(b_num)) => {
            // Compare numbers with tolerance for floats
            match (a_num.as_f64(), b_num.as_f64()) {
                (Some(a_f), Some(b_f)) => {
                    // Both are floats
                    if a_f.is_infinite() && b_f.is_infinite() {
                        a_f.is_sign_positive() == b_f.is_sign_positive()
                    } else if a_f.is_nan() && b_f.is_nan() {
                        true
                    } else {
                        let abs_diff = (a_f - b_f).abs();
                        let max_abs = a_f.abs().max(b_f.abs());
                        // Use relative tolerance for large numbers, absolute for small
                        abs_diff <= FLOAT_TOLERANCE || abs_diff <= max_abs * RELATIVE_TOLERANCE
                    }
                }
                _ => a_num == b_num, // Integer comparison
            }
        }
        (serde_json::Value::Array(a_arr), serde_json::Value::Array(b_arr)) => {
            a_arr.len() == b_arr.len()
                && a_arr
                    .iter()
                    .zip(b_arr.iter())
                    .all(|(a, b)| json_eq_with_float_tolerance(a, b))
        }
        (serde_json::Value::Object(a_obj), serde_json::Value::Object(b_obj)) => {
            a_obj.len() == b_obj.len()
                && a_obj
                    .iter()
                    .all(|(k, v)| b_obj.get(k).map_or(false, |b_v| json_eq_with_float_tolerance(v, b_v)))
        }
        _ => a == b, // For null, bool, string, use exact equality
    }
}

fn get_tester_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_cases(suite: &str) -> Result<Vec<TestCase>, String> {
    let testcases_path = get_tester_dir().join(format!("tests/{}.json", suite));
    let testcases_json = fs::read_to_string(&testcases_path).map_err(|e| format!("read testcases: {}", e))?;
    serde_json::from_str(&testcases_json).map_err(|e| format!("parse testcases: {}", e))
}

fn launch_test_binary(suite: &str, lang: Language) -> Result<Child, String> {
    println!("==> Launching {} test binary for suite '{}'", lang.as_str(), suite);

    let tester_dir = get_tester_dir();
    let output_dir = tester_dir.join("temp");
    fs::create_dir_all(&output_dir).map_err(|e| format!("create dir: {}", e))?;

    let schema_path = tester_dir.join(format!("tests/{}.vol", suite));
    if !schema_path.exists() {
        return Err(format!("Schema not found: {}", schema_path.display()));
    }

    // Load test cases to get type list
    let test_cases = load_test_cases(suite)?;

    // Extract unique type names from test cases
    let mut type_names: Vec<String> = test_cases
        .iter()
        .map(|tc| tc.r#type.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    type_names.sort();

    // Compile schema
    println!("  Compiling schema...");
    let lang_flag = match lang {
        Language::Rust => "rust",
        Language::Go => "go",
        Language::Typescript => "typescript",
    };

    let generated_path = output_dir.join(format!(
        "{}_generated.{}",
        suite,
        match lang {
            Language::Go => "go",
            Language::Typescript => "ts",
            Language::Rust => "rs",
        }
    ));

    let mut compile_args = vec![
        "run",
        "--bin",
        "volexc",
        "--",
        "--input",
        schema_path.to_str().unwrap(),
        "--output",
        "-",
        "--lang",
        lang_flag,
    ];

    // For Rust, enable serde derives
    if lang == Language::Rust {
        compile_args.push("--serde");
        compile_args.push("always");
    }

    let compile_output = Command::new("cargo")
        .args(&compile_args)
        .output()
        .map_err(|e| format!("failed to run compiler: {}", e))?;

    if !compile_output.status.success() {
        eprintln!("Compiler stderr:\n{}", String::from_utf8_lossy(&compile_output.stderr));
        return Err("schema compilation failed".to_string());
    }

    fs::write(&generated_path, &compile_output.stdout).map_err(|e| format!("failed to write generated code: {}", e))?;

    match lang {
        Language::Rust => lang_rust::launch(suite, &output_dir, &generated_path, &type_names),
        Language::Go => lang_go::launch(suite, &output_dir, &generated_path, &type_names),
        Language::Typescript => lang_typescript::launch(suite, &output_dir, &generated_path, &type_names),
    }
}

fn run_tests(suite: &str, lang: Language, child: Child, regen_hex: bool) -> Result<(), String> {
    println!("==> Testing {} implementation for suite '{}'", lang.as_str(), suite);

    let mut test_cases = load_test_cases(suite)?;

    println!("==> Loaded {} test cases", test_cases.len());

    let mut test_bin = TestBinary { process: child };

    let mut passed = 0;
    let mut failed = 0;
    let mut regenerated = 0;

    for (i, tc) in test_cases.iter_mut().enumerate() {
        // Check if this test should be skipped for this language
        if tc.skip_languages.contains(&lang.as_str().to_string()) {
            continue;
        }

        let hex_clean = tc.hex.replace(" ", "");

        if regen_hex {
            // Regenerate hex from JSON
            print!("  [{}] Regenerating hex for {}: ", i, tc.description);
            match test_bin.encode(&tc.r#type, tc.json.clone()) {
                Ok(new_hex) => {
                    if new_hex != hex_clean {
                        tc.hex = new_hex;
                        regenerated += 1;
                        println!("✓ (updated)");
                    } else {
                        println!("✓ (unchanged)");
                    }
                }
                Err(e) => {
                    println!("✗");
                    println!("    Error: {}", e);
                }
            }
        } else {
            // Test decode
            print!("  [{}] Decode {}: ", i, tc.description);
            match test_bin.decode(&tc.r#type, &hex_clean) {
                Ok(decoded_json) => {
                    if json_eq_with_float_tolerance(&decoded_json, &tc.json) {
                        println!("✓");
                        passed += 1;
                    } else {
                        println!("✗");
                        println!("    Expected: {}", serde_json::to_string_pretty(&tc.json).unwrap());
                        println!("    Got:      {}", serde_json::to_string_pretty(&decoded_json).unwrap());
                        failed += 1;
                    }
                }
                Err(e) => {
                    println!("✗ (error)");
                    println!("    Error: {}", e);
                    failed += 1;
                }
            }

            // Test encode
            print!("  [{}] Encode {}: ", i, tc.description);
            match test_bin.encode(&tc.r#type, tc.json.clone()) {
                Ok(encoded_hex) => {
                    if tc.nondeterministic_encode {
                        // For nondeterministic encoding, verify by doing a round-trip:
                        // encode -> decode -> compare JSON
                        match test_bin.decode(&tc.r#type, &encoded_hex) {
                            Ok(roundtrip_json) => {
                                if json_eq_with_float_tolerance(&roundtrip_json, &tc.json) {
                                    println!("✓");
                                    passed += 1;
                                } else {
                                    println!("✗");
                                    println!("    Expected: {}", serde_json::to_string_pretty(&tc.json).unwrap());
                                    println!(
                                        "    Got:      {}",
                                        serde_json::to_string_pretty(&roundtrip_json).unwrap()
                                    );
                                    failed += 1;
                                }
                            }
                            Err(e) => {
                                println!("✗ (decode error)");
                                println!("    Error: {}", e);
                                failed += 1;
                            }
                        }
                    } else {
                        if encoded_hex == hex_clean {
                            println!("✓");
                            passed += 1;
                        } else {
                            println!("✗");
                            println!("    Expected: {}", hex_clean);
                            println!("    Got:      {}", encoded_hex);
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("✗ (error)");
                    println!("    Error: {}", e);
                    failed += 1;
                }
            }
        }
    }

    if regen_hex {
        if regenerated > 0 {
            println!();
            let testcases_path = get_tester_dir().join(format!("tests/{}.json", suite));
            println!(
                "==> Regenerated {} hex values, saving to {}",
                regenerated,
                testcases_path.display()
            );
            let updated_json =
                serde_json::to_string_pretty(&test_cases).map_err(|e| format!("serialize updated testcases: {}", e))?;
            fs::write(&testcases_path, updated_json).map_err(|e| format!("write testcases: {}", e))?;
            println!("==> Test cases updated successfully!");
        } else {
            println!();
            println!("==> All hex values are correct, no changes needed");
        }
    } else {
        println!();
        println!("==> Results: {} passed, {} failed", passed, failed);

        if failed > 0 {
            println!();
            println!("Hint: Run with --regen-hex to automatically fix test data");
            return Err(format!("{} tests failed", failed));
        }
    }

    Ok(())
}

fn discover_suites() -> Result<Vec<String>, String> {
    let tests_dir = get_tester_dir().join("tests");
    let mut suites = Vec::new();

    let entries = fs::read_dir(&tests_dir).map_err(|e| format!("failed to read tests directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read entry: {}", e))?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("vol") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                suites.push(stem.to_string());
            }
        }
    }

    suites.sort();
    Ok(suites)
}

fn run_suite(suite: &str, lang: Language, regen_hex: bool) -> Result<(), String> {
    let child = match launch_test_binary(suite, lang) {
        Ok(child) => {
            println!("==> Launch successful");
            println!();
            child
        }
        Err(e) => {
            eprintln!("Error launching test binary: {}", e);
            return Err(format!("Failed to launch {}", suite));
        }
    };

    run_tests(suite, lang, child, regen_hex)
}

fn main() {
    let cli = Cli::parse();

    let suites = match &cli.suite {
        Some(suite) => vec![suite.clone()],
        None => match discover_suites() {
            Ok(suites) => {
                if suites.is_empty() {
                    eprintln!("No test suites found in ./tests/");
                    std::process::exit(1);
                }
                println!("Running all {} test suites: {}", suites.len(), suites.join(", "));
                println!();
                suites
            }
            Err(e) => {
                eprintln!("Failed to discover test suites: {}", e);
                std::process::exit(1);
            }
        },
    };

    let mut failed_suites = Vec::new();

    for (i, suite) in suites.iter().enumerate() {
        if suites.len() > 1 {
            println!("════════════════════════════════════════════════════════════════");
            println!("  Suite {}/{}: {}", i + 1, suites.len(), suite);
            println!("════════════════════════════════════════════════════════════════");
            println!();
        }

        match run_suite(suite, cli.lang, cli.regen_hex) {
            Ok(()) => {
                if !cli.regen_hex && suites.len() == 1 {
                    println!();
                    println!("✅ All tests passed!");
                }
            }
            Err(e) => {
                if suites.len() > 1 {
                    eprintln!();
                    eprintln!("❌ Suite '{}' failed: {}", suite, e);
                    failed_suites.push(suite.clone());
                } else {
                    eprintln!();
                    eprintln!("❌ {}", e);
                    std::process::exit(1);
                }
            }
        }

        if suites.len() > 1 {
            println!();
        }
    }

    if suites.len() > 1 && !cli.regen_hex {
        println!("════════════════════════════════════════════════════════════════");
        println!("  SUMMARY");
        println!("════════════════════════════════════════════════════════════════");

        if failed_suites.is_empty() {
            println!();
            println!("✅ All {} test suites passed!", suites.len());
        } else {
            println!();
            println!("❌ {}/{} test suites failed:", failed_suites.len(), suites.len());
            for suite in &failed_suites {
                println!("   - {}", suite);
            }
            std::process::exit(1);
        }
    }
}
