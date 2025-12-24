use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};

pub fn launch(suite: &str, output_dir: &Path, generated_path: &Path, type_names: &[String]) -> Result<Child, String> {
    let test_dir = output_dir.join(format!("{}_testbin_go", suite));
    fs::create_dir_all(&test_dir).map_err(|e| format!("create test dir: {}", e))?;

    // Create go.mod
    let go_mod = format!(
        r#"module {}_testbin

go 1.21

replace github.com/lolserialize/runtime => ../../../lolserialize-go-runtime
"#,
        suite
    );
    fs::write(test_dir.join("go.mod"), go_mod).map_err(|e| format!("write go.mod: {}", e))?;

    // Copy generated Go code
    let generated_code = fs::read_to_string(generated_path).map_err(|e| format!("read generated code: {}", e))?;
    fs::write(test_dir.join("generated.go"), generated_code).map_err(|e| format!("write generated.go: {}", e))?;

    // Generate encode/decode switch cases
    let encode_cases = type_names
        .iter()
        .map(|t| {
            format!(
                r#"	case "{}":
		var val {}
		if err := json.Unmarshal(jsonData, &val); err != nil {{
			return "", fmt.Errorf("failed to deserialize: %v", err)
		}}
		val.Encode(&buf)"#,
                t, t
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let decode_cases = type_names
        .iter()
        .map(|t| {
            format!(
                r#"	case "{}":
		val, err := Decode{}(&bytes)
		if err != nil {{
			return nil, fmt.Errorf("decode failed: %v", err)
		}}
		return val, nil"#,
                t, t
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Create main.go with test binary protocol implementation
    let main_go = format!(
        r#"package main

import (
	"bufio"
	"encoding/hex"
	"fmt"
	"os"

	jsonv1 "encoding/json"
	"encoding/json/v2"
)

type Request struct {{
	Cmd  string          `json:"cmd"`
	Type string          `json:"type"`
	Json jsonv1.RawMessage `json:"json"`
	Hex  string          `json:"hex"`
}}

type TestResponse struct {{
	Ok     bool        `json:"ok"`
	Result interface{{}} `json:"result"`
	Error  string      `json:"error"`
}}

func encodeType(typeName string, jsonData []byte) (string, error) {{
	buf := []byte{{}}

	switch typeName {{
{}
	default:
		return "", fmt.Errorf("unknown type: %s", typeName)
	}}

	return hex.EncodeToString(buf), nil
}}

func decodeType(typeName string, hexStr string) (interface{{}}, error) {{
	bytes, err := hex.DecodeString(hexStr)
	if err != nil {{
		return nil, fmt.Errorf("invalid hex: %v", err)
	}}

	switch typeName {{
{}
	default:
		return nil, fmt.Errorf("unknown type: %s", typeName)
	}}
}}

func main() {{
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {{
		line := scanner.Text()

		var req Request
		if err := json.Unmarshal([]byte(line), &req); err != nil {{
			resp := TestResponse{{Ok: false, Error: fmt.Sprintf("invalid request: %v", err)}}
			respJson, _ := json.Marshal(resp)
			fmt.Println(string(respJson))
			continue
		}}

		var resp TestResponse
		switch req.Cmd {{
		case "encode":
			result, err := encodeType(req.Type, req.Json)
			if err != nil {{
				resp = TestResponse{{Ok: false, Error: err.Error()}}
			}} else {{
				resp = TestResponse{{Ok: true, Result: result}}
			}}
		case "decode":
			result, err := decodeType(req.Type, req.Hex)
			if err != nil {{
				resp = TestResponse{{Ok: false, Error: err.Error()}}
			}} else {{
				resp = TestResponse{{Ok: true, Result: result}}
			}}
		default:
			resp = TestResponse{{Ok: false, Error: fmt.Sprintf("unknown command: %s", req.Cmd)}}
		}}

		respJson, err := json.Marshal(resp)
		if err != nil {{
			resp = TestResponse{{Ok: false, Error: fmt.Sprintf("failed to serialize response: %v", err)}}
			respJson, _ = json.Marshal(resp)
		}}
		fmt.Println(string(respJson))
	}}
}}
"#,
        encode_cases, decode_cases
    );

    fs::write(test_dir.join("main.go"), main_go).map_err(|e| format!("write main.go: {}", e))?;

    // Run go mod tidy to resolve dependencies
    println!("  Running go mod tidy...");
    let tidy_output = Command::new("go")
        .args(&["mod", "tidy"])
        .current_dir(&test_dir)
        .output()
        .map_err(|e| format!("go mod tidy: {}", e))?;

    if !tidy_output.status.success() {
        eprintln!("Tidy stderr:\n{}", String::from_utf8_lossy(&tidy_output.stderr));
        return Err("go mod tidy failed".to_string());
    }

    // Launch with go run (builds if needed, then runs)
    println!("  Launching with go run...");
    let child = Command::new("go")
        .args(&["run", "."])
        .current_dir(&test_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("GOEXPERIMENT", "jsonv2")
        .spawn()
        .map_err(|e| format!("go run: {}", e))?;

    Ok(child)
}
