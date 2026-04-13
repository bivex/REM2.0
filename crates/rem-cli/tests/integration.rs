use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
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
    
    cmd.arg("extract")
       .arg("--file").arg(main_rs_path.to_str().unwrap())
       .arg("--start").arg(start.to_string())
       .arg("--end").arg(end.to_string())
       .arg("--name").arg("compute_accum")
       .arg("--project-root").arg(project_root.to_str().unwrap())
       .arg("--json");

    let assert = cmd.assert().success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    println!("CLI Output:\n{}", stdout);

    // Read the modified file
    let modified_source = fs::read_to_string(&main_rs_path).unwrap();
    println!("Modified Source:\n{}", modified_source);

    assert!(modified_source.contains("fn compute_accum"));
    assert!(modified_source.contains("compute_accum("));
    
    // In our dummy implementation, we expect extraction to occur but the parameters 
    // will be empty because we bypassed the RA adapter analysis. We'll improve this next.
}
