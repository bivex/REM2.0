use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Helper to copy the fixture directory to a temporary folder so tests don't
/// overwrite the original files when running cargo check + modifying source.
fn copy_fixture_to_temp(fixture_name: &str) -> TempDir {
    let temp_dir = tempfile::Builder::new()
        .prefix("rem3-test-")
        .tempdir()
        .expect("failed to create temp dir");

    let mut fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fixture_path.pop(); // pop rem-cli
    fixture_path.pop(); // pop crates
    fixture_path.push("tests");
    fixture_path.push("fixtures");
    fixture_path.push(fixture_name);

    if !fixture_path.exists() {
        panic!("Fixture not found: {}", fixture_path.display());
    }

    let mut options = fs_extra::dir::CopyOptions::new();
    options.copy_inside = true;
    fs_extra::dir::copy(&fixture_path, temp_dir.path(), &options).expect("failed to copy fixture");

    temp_dir
}

fn get_extraction_range(source: &str, start_marker: &str, end_marker: &str) -> (u32, u32) {
    let start_idx = source.find(start_marker).expect("start marker not found") + start_marker.len();
    // skip the newline after the marker
    let start_idx = if source[start_idx..].starts_with("\r\n") { start_idx + 2 } else { start_idx + 1 };

    let end_idx = source.find(end_marker).expect("end marker not found");
    // step back to before the newline preceding the end marker
    let end_idx = if source[..end_idx].ends_with("\r\n") { end_idx - 2 } else { end_idx - 1 };

    (start_idx as u32, end_idx as u32)
}

#[test]
fn test_basic_extract() {
    let temp_dir = copy_fixture_to_temp("basic");
    let project_root = temp_dir.path().join("basic");
    let main_rs_path = project_root.join("src").join("main.rs");

    let source = fs::read_to_string(&main_rs_path).unwrap();
    let (start, end) = get_extraction_range(&source, "// -- EXTRACT START --", "// -- EXTRACT END --");

    let mut cmd = Command::cargo_bin("rem").expect("failed to find rem binary");
    cmd.env("RUST_LOG", "info");
    
    cmd.arg("extract")
       .arg("--file").arg(main_rs_path.to_str().unwrap())
       .arg("--start").arg(start.to_string())
       .arg("--end").arg(end.to_string())
       .arg("--name").arg("compute_accum")
       .arg("--project-root").arg(project_root.to_str().unwrap())
       .arg("--json");

    let assert = cmd.assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8(output.stdout.clone()).unwrap();
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    println!("CLI Output (stdout):\n{}", stdout);
    println!("CLI Output (stderr):\n{}", stderr);

    // Read the modified file
    let modified_source = fs::read_to_string(&main_rs_path).unwrap();
    println!("Modified Source:\n{}", modified_source);

    assert!(modified_source.contains("fn compute_accum"));
    assert!(modified_source.contains("compute_accum("));
    
    // In our dummy implementation, we expect extraction to occur but the parameters
    // will be empty because we bypassed the RA adapter analysis. We'll improve this next.
}

// ── Horror-refactor levels ──────────────────────────────────────────────────

/// Helper: run extraction on a horror-refactor fixture and report outcome.
/// Returns `(success, json_response_or_None, stderr)`.
fn run_horror_extraction(
    fixture_file: &str,
    function_name: &str,
) -> (bool, Option<serde_json::Value>, String) {
    let temp_dir = copy_fixture_to_temp("horror_refactor");
    let project_root = temp_dir.path().join("horror_refactor");
    let file_path = project_root.join("src").join(fixture_file);

    let source = fs::read_to_string(&file_path).unwrap();
    let (start, end) = get_extraction_range(&source, "// --- EXTRACT START ---", "// --- EXTRACT END ---");

    let extracted_text = &source[start as usize..end as usize];
    println!("[{}] extracting bytes {}..{}:\n{}", fixture_file, start, end, extracted_text);

    let mut cmd = Command::cargo_bin("rem").unwrap();
    cmd.env("RUST_LOG", "info");

    cmd.arg("extract")
       .arg("--file").arg(file_path.to_str().unwrap())
       .arg("--start").arg(start.to_string())
       .arg("--end").arg(end.to_string())
       .arg("--name").arg(function_name)
       .arg("--project-root").arg(project_root.to_str().unwrap())
       .arg("--json");

    let output = cmd.output().expect("failed to execute rem");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    println!("[{}] stderr:\n{}", fixture_file, stderr);

    if stdout.trim().is_empty() {
        println!("[{}] no JSON output (CLI errored)", fixture_file);
        return (false, None, stderr);
    }

    match serde_json::from_str::<serde_json::Value>(&stdout) {
        Ok(res) => {
            let success = res["success"].as_bool().unwrap_or(false);
            if success {
                let src = res["new_file_content"].as_str().unwrap_or("");
                println!("[{}] extracted source:\n{}", fixture_file, src);
            } else {
                println!("[{}] extraction failed: {}", fixture_file, res["error"]);
            }
            (success, Some(res), stderr)
        }
        Err(e) => {
            println!("[{}] JSON parse error: {} — raw output:\n{}", fixture_file, e, stdout);
            (false, None, stderr)
        }
    }
}

/// Level 01: Mutability & Ownership stress test.
/// Multiple mutable borrows, partial moves, variable shadowing.
#[test]
fn test_horror_level_01_mut_ownership() {
    let (success, res, stderr) = run_horror_extraction(
        "level_01_mut_ownership.rs",
        "extracted_block",
    );

    println!("Level 01 outcome: success={}", success);

    if success {
        let val = res.unwrap();
        let new_src = val["new_file_content"].as_str().unwrap();
        assert!(new_src.contains("fn extracted_block"), "extracted function not found in output");
    } else {
        println!("Level 01 FAILED — tool could not handle this level.");
        println!("stderr:\n{}", stderr);
        // Don't panic — this is the point of the test: discover where it breaks.
    }
}

/// Level 02: Control-flow stress test.
/// break / continue targeting the outer loop, conditional returns, error handling.
#[test]
fn test_horror_level_02_control_flow() {
    let (success, res, stderr) = run_horror_extraction(
        "level_02_control_flow.rs",
        "extracted_block",
    );

    println!("Level 02 outcome: success={}", success);

    if success {
        let val = res.unwrap();
        let new_src = val["new_file_content"].as_str().unwrap();
        assert!(new_src.contains("fn extracted_block"), "extracted function not found in output");
    } else {
        println!("Level 02 FAILED — tool could not handle this level.");
        println!("stderr:\n{}", stderr);
    }
}

/// Level 03: Lifetime labyrinth.
/// Multiple input lifetimes, returning a reference, lifetime elision vs explicit.
#[test]
fn test_horror_level_03_lifetimes() {
    let (success, res, stderr) = run_horror_extraction(
        "level_03_lifetimes.rs",
        "extracted_block",
    );

    println!("Level 03 outcome: success={}", success);

    if success {
        let val = res.unwrap();
        let new_src = val["new_file_content"].as_str().unwrap();
        assert!(new_src.contains("fn extracted_block"), "extracted function not found in output");
    } else {
        println!("Level 03 FAILED — tool could not handle this level.");
        println!("stderr:\n{}", stderr);
    }
}

#[test]
fn test_generic_extract() {
    let temp_dir = copy_fixture_to_temp("basic"); // reuse basic fixture structure
    let project_root = temp_dir.path().join("basic");
    let main_rs_path = project_root.join("src").join("main.rs");

    let project_src = r#"
trait MyTrait {}
impl MyTrait for i32 {}

fn print_val<T: MyTrait + Copy>(val: T) {
    // start
    let _x = val;
    // end
}

fn main() {
    print_val(42);
}
"#;
    
    fs::write(&main_rs_path, project_src).unwrap();
    
    let start = project_src.find("let").unwrap();
    let end = project_src.find("// end").unwrap();
    
    println!("Selected text for extraction: {:?}", &project_src[start..end]);
    println!("Offsets: start={}, end={}", start, end);
    
    let mut cmd = Command::cargo_bin("rem").expect("failed to find rem binary");
    cmd.env("RUST_LOG", "info");
    
    cmd.arg("extract")
       .arg("--file").arg(main_rs_path.to_str().unwrap())
       .arg("--start").arg(start.to_string())
       .arg("--end").arg(end.to_string())
       .arg("--name").arg("do_print")
       .arg("--project-root").arg(project_root.to_str().unwrap())
       .arg("--json");

    let output = cmd.output().expect("failed to execute command");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    println!("Stdout: {}", stdout);
    println!("Stderr: {}", stderr);
    
    let res: serde_json::Value = serde_json::from_str(&stdout).expect("failed to parse JSON");
    
    assert!(res["success"].as_bool().unwrap(), "Extraction failed: {}", res["error"]);
    
    let new_src = res["new_file_content"].as_str().unwrap();
    println!("Generic Source:\n{}", new_src);
    
    // Check if T: MyTrait is in the extracted function
    assert!(
        new_src.contains("fn do_print<T: MyTrait>(val: T)"),
        "Expected `fn do_print<T: MyTrait>(val: T)` in output, got:\n{new_src}"
    );
}
