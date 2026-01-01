use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};

pub fn launch(suite: &str, output_dir: &Path, generated_path: &Path, type_names: &[String]) -> Result<Child, String> {
    let test_dir = output_dir.join(format!("{}_testbin_typescript", suite));
    fs::create_dir_all(&test_dir).map_err(|e| format!("create test dir: {}", e))?;

    // Create package.json with volex dependency pointing to runtime-typescript
    let package_json = r#"{
  "name": "volex-testbin",
  "version": "1.0.0",
  "type": "module",
  "dependencies": {
    "volex": "file:../../../runtime-typescript"
  }
}
"#;
    fs::write(test_dir.join("package.json"), package_json).map_err(|e| format!("write package.json: {}", e))?;

    // Install dependencies if needed
    if !test_dir.join("node_modules").exists() {
        let install_output = Command::new("npm")
            .args(["install"])
            .current_dir(&test_dir)
            .output()
            .map_err(|e| format!("npm install: {}", e))?;

        if !install_output.status.success() {
            return Err(format!(
                "npm install failed:\n{}",
                String::from_utf8_lossy(&install_output.stderr)
            ));
        }
    }

    // Copy generated TypeScript code
    let generated_code = fs::read_to_string(generated_path).map_err(|e| format!("read generated code: {}", e))?;
    fs::write(test_dir.join("generated.ts"), generated_code).map_err(|e| format!("write generated.ts: {}", e))?;

    // Generate encode/decode switch cases
    let encode_cases = type_names
        .iter()
        .map(|t| {
            format!(
                r#"    case "{}":
      const val{} = convertJsonValue(req.json);
      const buf{}: Uint8Array[] = [];
      encode{}(val{}, buf{});
      return {{ ok: true, result: Array.from(runtime.flattenBuf(buf{})).map(b => b.toString(16).padStart(2, '0')).join('') }};"#,
                t, t, t, t, t, t, t
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let decode_cases = type_names
        .iter()
        .map(|t| {
            format!(
                r#"    case "{}":
      const bytes{} = req.hex ? new Uint8Array(req.hex.match(/.{{1,2}}/g)!.map(byte => parseInt(byte, 16))) : new Uint8Array(0);
      const buf{} = new runtime.Buf(bytes{});
      const result{} = decode{}(buf{});
      return {{ ok: true, result: convertToJson(result{}) }};"#,
                t, t, t, t, t, t, t, t
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Create main.ts with test binary protocol implementation
    let main_ts = format!(
        r#"import * as runtime from "volex";
import {{
{}
}} from "./generated.js";
import * as readline from "readline";

interface Request {{
  cmd: "encode" | "decode";
  type: string;
  json?: any;
  hex?: string;
}}

interface Response {{
  ok: boolean;
  result?: any;
  error?: string;
}}

// Convert JSON value to proper TypeScript types (Objects to Maps for map fields only)
function convertJsonValue(val: any): any {{
  if (val === null || val === undefined) return val;
  if (Array.isArray(val)) {{
    return val.map(v => convertJsonValue(v));
  }}
  if (typeof val === 'object') {{
    const result: any = {{}};
    for (const [k, v] of Object.entries(val)) {{
      // For 'values' fields - assume deeply nested maps (e.g., {{string: {{string: u32}}}}, [{{string: u32}}])
      if (k === 'values' && v !== null && typeof v === 'object' && !('$tag' in v)) {{
        result[k] = convertMapFieldDeep(v);
      }}
      // For 'map' fields or fields ending in '_map' - assume shallow maps (values might be structs)
      else if ((k === 'map' || k.endsWith('_map')) && v !== null && typeof v === 'object' && !('$tag' in v)) {{
        if (Array.isArray(v)) {{
          result[k] = v.map((elem: any) => convertJsonValue(elem));
        }} else {{
          result[k] = convertObjectToMapShallow(v);
        }}
      }} else if (k === '$value' && val.$tag === 'Map' && v !== null && typeof v === 'object' && !Array.isArray(v)) {{
        // For unions with tag="Map", convert $value to Map
        // Don't recursively convert values - they might be structs
        result[k] = convertObjectToMapShallow(v);
      }} else {{
        result[k] = convertJsonValue(v);
      }}
    }}
    return result;
  }}
  return val;
}}

// Convert a map field value deeply - handles maps, arrays of maps, nested maps, etc.
// This assumes all nested objects are maps (not structs)
function convertMapFieldDeep(val: any): any {{
  if (val === null || val === undefined) return val;
  if (Array.isArray(val)) {{
    return val.map((elem: any) => convertMapFieldDeep(elem));
  }}
  if (typeof val === 'object') {{
    return convertObjectToMapDeep(val);
  }}
  return val;
}}

// Convert a JSON object to a Map, recursively converting nested objects to Maps
function convertObjectToMapDeep(obj: any): Map<any, any> {{
  const map = new Map();
  for (const [mk, mv] of Object.entries(obj)) {{
    if (mv !== null && typeof mv === 'object' && !Array.isArray(mv) && !('$tag' in mv)) {{
      map.set(mk, convertObjectToMapDeep(mv));
    }} else if (Array.isArray(mv)) {{
      map.set(mk, convertMapFieldDeep(mv));
    }} else {{
      map.set(mk, convertJsonValue(mv));
    }}
  }}
  return map;
}}

// Convert a JSON object to a Map without recursively converting nested objects
// (used when values might be structs, not nested maps)
function convertObjectToMapShallow(obj: any): Map<any, any> {{
  const map = new Map();
  for (const [mk, mv] of Object.entries(obj)) {{
    map.set(mk, convertJsonValue(mv));
  }}
  return map;
}}

// Convert result back to JSON-serializable form (Maps to objects)
function convertToJson(val: any): any {{
  if (val === null || val === undefined) return val;
  if (val instanceof Map) {{
    const obj: any = {{}};
    for (const [k, v] of val.entries()) {{
      obj[k] = convertToJson(v);
    }}
    return obj;
  }}
  if (Array.isArray(val)) {{
    return val.map(convertToJson);
  }}
  if (typeof val === 'object') {{
    const result: any = {{}};
    for (const [k, v] of Object.entries(val)) {{
      result[k] = convertToJson(v);
    }}
    return result;
  }}
  return val;
}}

function processRequest(req: Request): Response {{
  try {{
    if (req.cmd === "encode") {{
      switch (req.type) {{
{}
        default:
          return {{ ok: false, error: `Unknown type: ${{req.type}}` }};
      }}
    }} else if (req.cmd === "decode") {{
      switch (req.type) {{
{}
        default:
          return {{ ok: false, error: `Unknown type: ${{req.type}}` }};
      }}
    }} else {{
      return {{ ok: false, error: `Unknown command: ${{req.cmd}}` }};
    }}
  }} catch (error) {{
    return {{ ok: false, error: String(error) }};
  }}
}}

// Read from stdin line by line
const rl = readline.createInterface({{
  input: process.stdin,
  output: process.stdout,
  terminal: false
}});

rl.on('line', (line: string) => {{
  if (line.trim()) {{
    const req = JSON.parse(line);
    const resp = processRequest(req);
    console.log(JSON.stringify(resp));
  }}
}});
"#,
        type_names
            .iter()
            .flat_map(|t| vec![format!("encode{}", t), format!("decode{}", t)])
            .collect::<Vec<_>>()
            .join(", "),
        encode_cases,
        decode_cases
    );

    fs::write(test_dir.join("main.ts"), main_ts).map_err(|e| format!("write main.ts: {}", e))?;

    // Create tsconfig.json
    let tsconfig = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ES2020",
    "moduleResolution": "node",
    "esModuleInterop": true,
    "strict": false,
    "skipLibCheck": true
  }
}
"#;
    fs::write(test_dir.join("tsconfig.json"), tsconfig).map_err(|e| format!("write tsconfig.json: {}", e))?;

    // Launch with tsx (TypeScript execution)
    println!("  Launching with tsx...");
    let child = Command::new("npx")
        .args(&["tsx", "main.ts"])
        .current_dir(&test_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("npx tsx: {}", e))?;

    Ok(child)
}
