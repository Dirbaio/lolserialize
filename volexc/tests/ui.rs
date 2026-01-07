use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedResult {
    Pass,
    Fail,
}

#[derive(Debug)]
struct TestConfig {
    // Map from language name to expected result (pass/fail)
    language_configs: HashMap<String, ExpectedResult>,
    // If true, only run for specifically configured languages
    // If false, run for default (rust) if no languages specified
    has_language_specific_config: bool,
}

impl TestConfig {
    fn parse(content: &str) -> Self {
        let mut language_configs = HashMap::new();
        let mut has_language_specific_config = false;
        let mut has_generic_directive = false;
        let mut generic_result = ExpectedResult::Fail;

        for line in content.lines() {
            let line = line.trim();
            if !line.starts_with("//") {
                continue;
            }
            let directive = line[2..].trim();

            // Check for language-specific directives: "rust:pass", "go:fail", etc.
            if let Some(colon_pos) = directive.find(':') {
                // Split at the first colon
                let (before_colon, after_colon) = directive.split_at(colon_pos);
                let lang = before_colon.trim();
                let result_str = after_colon[1..].trim(); // Skip the colon itself

                // Only treat it as a language directive if the part before colon
                // looks like a language name (no spaces, lowercase/alpha)
                // and the part after is "pass" or "fail"
                if !lang.is_empty()
                    && lang.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    && (result_str == "pass" || result_str == "fail")
                {
                    let expected = if result_str == "pass" {
                        ExpectedResult::Pass
                    } else {
                        ExpectedResult::Fail
                    };
                    language_configs.insert(lang.to_string(), expected);
                    has_language_specific_config = true;
                }
            } else if directive == "pass" {
                has_generic_directive = true;
                generic_result = ExpectedResult::Pass;
            } else if directive == "fail" {
                has_generic_directive = true;
                generic_result = ExpectedResult::Fail;
            }
        }

        // If we have a generic directive and no language-specific configs,
        // apply it to the default language (rust)
        if has_generic_directive && !has_language_specific_config {
            language_configs.insert("rust".to_string(), generic_result);
        }

        // If no directives at all, default to "rust:fail"
        if language_configs.is_empty() {
            language_configs.insert("rust".to_string(), ExpectedResult::Fail);
        }

        TestConfig {
            language_configs,
            has_language_specific_config,
        }
    }

    fn languages(&self) -> Vec<String> {
        let mut langs: Vec<_> = self.language_configs.keys().cloned().collect();
        langs.sort();
        langs
    }

    fn expected_result(&self, language: &str) -> ExpectedResult {
        self.language_configs
            .get(language)
            .copied()
            .unwrap_or(ExpectedResult::Fail)
    }
}

#[test]
fn ui_tests() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let ui_dir = Path::new(manifest_dir).join("tests/ui");

    // Build volexc first
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(manifest_dir)
        .status()
        .expect("Failed to build volexc");

    assert!(status.success(), "Failed to build volexc");

    let volexc_path = Path::new(manifest_dir).parent().unwrap().join("target/release/volexc");

    // Collect all .vol files
    let mut test_files: Vec<PathBuf> = fs::read_dir(&ui_dir)
        .expect("Failed to read ui directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()? == "vol" { Some(path) } else { None }
        })
        .collect();

    test_files.sort();

    assert!(!test_files.is_empty(), "No UI test files found in tests/ui/");

    let mut failed_tests = Vec::new();

    for test_file in test_files {
        let test_name = test_file.file_stem().unwrap().to_str().unwrap();

        // Parse test configuration from comments
        let content = fs::read_to_string(&test_file).expect("Failed to read test file");
        let config = TestConfig::parse(&content);

        // Run test for each configured language
        for language in config.languages() {
            let expected_result = config.expected_result(&language);

            // Determine output file name
            let output_file = if config.has_language_specific_config {
                // Language-specific: foo.rust.out, foo.go.out, etc.
                test_file.with_extension(format!("{}.out", language))
            } else {
                // No language specified: foo.out (default rust)
                test_file.with_extension("out")
            };

            // Run volexc on the test file with the specified language
            let output = Command::new(&volexc_path)
                .arg("--input")
                .arg(&test_file)
                .arg("--output")
                .arg("-") // Output to stdout
                .arg("--lang")
                .arg(&language)
                .output()
                .expect("Failed to execute volexc");

            // Check if the result matches expectations
            let compilation_succeeded = output.status.success();
            let expected_success = expected_result == ExpectedResult::Pass;

            if compilation_succeeded != expected_success {
                let test_label = if config.has_language_specific_config {
                    format!("{}:{}", test_name, language)
                } else {
                    test_name.to_string()
                };

                let error_msg = if expected_success {
                    format!("Expected compilation to pass but it failed")
                } else {
                    format!("Expected compilation to fail but it passed")
                };

                failed_tests.push((test_label, error_msg, String::new()));
                continue;
            }

            let test_label = if config.has_language_specific_config {
                format!("{}:{}", test_name, language)
            } else {
                test_name.to_string()
            };

            // For passing tests, just verify compilation succeeded and skip output comparison
            if expected_result == ExpectedResult::Pass {
                println!("✓ {}", test_label);
                continue;
            }

            // For failing tests, compare the error output
            // Combine stdout and stderr for comparison
            let mut actual_output = String::new();
            if !output.stdout.is_empty() {
                actual_output.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                if !actual_output.is_empty() {
                    actual_output.push('\n');
                }
                actual_output.push_str(&String::from_utf8_lossy(&output.stderr));
            }

            // Normalize the output:
            // 1. Replace absolute paths with relative paths
            // 2. Normalize line endings
            let actual_output = normalize_output(&actual_output, &test_file);

            // Check if UPDATE mode is enabled
            let update_mode = std::env::var("UPDATE").unwrap_or_default() == "1";

            if update_mode {
                // UPDATE=1: Create or update .out file and always pass
                fs::write(&output_file, &actual_output).expect("Failed to write .out file");
                if output_file.exists() {
                    println!("✓ {} (updated)", test_label);
                } else {
                    println!("✓ {} (created)", test_label);
                }
            } else {
                // Normal mode: compare with expected output
                if !output_file.exists() {
                    failed_tests.push((
                        test_label,
                        format!("Missing .out file: {}", output_file.display()),
                        actual_output,
                    ));
                    continue;
                }

                let expected_output = fs::read_to_string(&output_file).expect("Failed to read .out file");

                // Compare outputs
                if actual_output.trim() != expected_output.trim() {
                    failed_tests.push((test_label, expected_output, actual_output));
                } else {
                    println!("✓ {}", test_label);
                }
            }
        }
    }

    // Report failures
    if !failed_tests.is_empty() {
        eprintln!("\n{} test(s) failed:\n", failed_tests.len());
        for (name, expected, actual) in &failed_tests {
            eprintln!("--- {} ---", name);
            eprintln!("Expected:");
            eprintln!("{}", expected);
            eprintln!("\nActual:");
            eprintln!("{}", actual);
            eprintln!();
        }
        panic!("{} UI test(s) failed", failed_tests.len());
    }
}

fn normalize_output(output: &str, test_file: &Path) -> String {
    let test_dir = test_file.parent().unwrap();
    let test_filename = test_file.file_name().unwrap().to_str().unwrap();

    output
        .lines()
        .map(|line| {
            // Replace absolute path with relative path
            let line = line.replace(test_file.to_str().unwrap(), test_filename);
            let line = line.replace(test_dir.to_str().unwrap(), "tests/ui");

            // Remove color codes (ANSI escape sequences)
            remove_ansi_codes(&line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn remove_ansi_codes(s: &str) -> String {
    // Simple ANSI escape code removal
    let mut result = String::new();
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip the escape sequence
            if chars.next() == Some('[') {
                // Skip until we find a letter (the command)
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}
