/// Integration tests for RainbowTerm profiles
///
/// This test suite discovers and runs all test files against their corresponding
/// profiles, displaying colored output for visual validation.
///
/// Test file structure:
/// tests/networking/{vendor}/{device}/{output_type}.txt
///
/// Maps to profiles:
/// - tests/networking/juniper/* -> juniper profile
/// - tests/networking/cisco/* -> cisco profile
/// - tests/networking/arista/* -> arista profile

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::io::Write;
use std::thread;

/// Test data structure
#[derive(Debug)]
struct TestCase {
    profile: String,
    device_type: String,
    test_file: PathBuf,
}

/// Discover all test files in the networking directory
fn discover_test_cases() -> Vec<TestCase> {
    let mut cases = Vec::new();
    let tests_dir = PathBuf::from("tests/networking");

    if !tests_dir.exists() {
        eprintln!("Warning: tests/networking directory not found");
        return cases;
    }

    // Iterate through vendors (juniper, cisco, arista, etc.)
    if let Ok(vendors) = fs::read_dir(&tests_dir) {
        for vendor_entry in vendors.flatten() {
            let vendor_path = vendor_entry.path();
            if !vendor_path.is_dir() {
                continue;
            }

            let vendor_name = vendor_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Iterate through device types (ex2300, srx, ios, etc.)
            if let Ok(devices) = fs::read_dir(&vendor_path) {
                for device_entry in devices.flatten() {
                    let device_path = device_entry.path();
                    if !device_path.is_dir() {
                        continue;
                    }

                    let device_name = device_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();

                    // Find all .conf files in device directory
                    if let Ok(files) = fs::read_dir(&device_path) {
                        for file_entry in files.flatten() {
                            let file_path = file_entry.path();
                            if file_path.extension().and_then(|s| s.to_str()) == Some("conf") {
                                cases.push(TestCase {
                                    profile: vendor_name.clone(),
                                    device_type: device_name.clone(),
                                    test_file: file_path,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    cases.sort_by(|a, b| a.test_file.cmp(&b.test_file));
    cases
}

/// Run rt with a test file and return the colored output
fn run_rt_colorizer(profile: &str, test_file: &PathBuf) -> Result<Vec<u8>, String> {
    let input = fs::read_to_string(test_file)
        .map_err(|e| format!("Failed to read test file: {}", e))?;

    let mut child = Command::new("rt")
        .arg("--profile")
        .arg(profile)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn rt: {}", e))?;

    // Take ownership of stdin to write in a separate thread
    let mut stdin = child.stdin.take().ok_or("Failed to open stdin")?;

    // Write stdin in a separate thread to avoid deadlock with large inputs
    let input_clone = input.clone();
    let stdin_thread = thread::spawn(move || {
        let _ = stdin.write_all(input_clone.as_bytes());
        drop(stdin); // Close stdin to signal EOF
    });

    // Wait for output with a timeout mechanism
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for rt: {}", e))?;

    // Wait for stdin thread to complete
    let _ = stdin_thread.join();

    if !output.status.success() {
        return Err(format!(
            "rt failed with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(output.stdout)
}

/// Helper to get relative path for display
fn relative_path(path: &PathBuf) -> String {
    path.to_string_lossy()
        .replace("tests/networking/", "")
        .to_string()
}

/// Profiles that are not yet implemented (skip in test_all_profiles)
const UNIMPLEMENTED_PROFILES: &[&str] = &["arista"];

#[test]
fn test_all_profiles() {
    println!("\n");
    println!("================================================================================");
    println!("RainbowTerm Profile Integration Tests");
    println!("================================================================================\n");

    // Filter out unimplemented profiles
    let test_cases: Vec<_> = discover_test_cases()
        .into_iter()
        .filter(|c| !UNIMPLEMENTED_PROFILES.contains(&c.profile.as_str()))
        .collect();

    if test_cases.is_empty() {
        println!("No test cases found. Expected structure:");
        println!("  tests/networking/{{vendor}}/{{device}}/{{test}}.txt");
        println!("\nExample:");
        println!("  tests/networking/juniper/ex2300/sample.txt");
        println!("  tests/networking/cisco/ios/sample.txt");
        return;
    }

    let mut passed = 0;
    let mut failed = 0;

    for test_case in test_cases {
        let relative = relative_path(&test_case.test_file);

        print!(
            "Testing: {} / {} ... ",
            test_case.profile, test_case.device_type
        );

        match run_rt_colorizer(&test_case.profile, &test_case.test_file) {
            Ok(output) => {
                println!("✓ OK");

                // Print the colored output with a border
                println!("────────────────────────────────────────────────────────────────────────────────");
                println!("File: {}", relative);
                println!("────────────────────────────────────────────────────────────────────────────────");

                // Print the colored output
                if let Ok(output_str) = String::from_utf8(output) {
                    println!("{}", output_str);
                } else {
                    println!("(binary/invalid UTF-8 output)");
                }

                println!("────────────────────────────────────────────────────────────────────────────────\n");
                passed += 1;
            }
            Err(e) => {
                println!("✗ FAILED: {}", e);
                failed += 1;
            }
        }
    }

    println!("================================================================================");
    println!("Test Results: {} passed, {} failed", passed, failed);
    println!("================================================================================\n");

    assert_eq!(
        failed, 0,
        "Some profile tests failed. See output above for details."
    );
}

/// Individual test for each major vendor (for easier filtering with cargo test --test integration_tests juniper)
#[test]
fn test_juniper_profiles() {
    println!("\n");
    println!("Testing Juniper profiles...");
    let cases: Vec<_> = discover_test_cases()
        .into_iter()
        .filter(|c| c.profile == "juniper")
        .collect();

    if cases.is_empty() {
        println!("Skipping: No Juniper test cases found in tests/networking/juniper/");
        println!("Expected structure: tests/networking/juniper/{{device}}/*.conf");
        return;
    }

    for case in cases {
        print!("  {} / {} ... ", case.profile, case.device_type);
        match run_rt_colorizer(&case.profile, &case.test_file) {
            Ok(_) => println!("✓"),
            Err(e) => panic!("Failed: {}", e),
        }
    }
}

#[test]
fn test_cisco_profiles() {
    println!("\n");
    println!("Testing Cisco profiles...");
    let cases: Vec<_> = discover_test_cases()
        .into_iter()
        .filter(|c| c.profile == "cisco")
        .collect();

    if cases.is_empty() {
        println!("Skipping: No Cisco test cases found in tests/networking/cisco/");
        println!("Expected structure: tests/networking/cisco/{{device}}/*.conf");
        return;
    }

    for case in cases {
        print!("  {} / {} ... ", case.profile, case.device_type);
        match run_rt_colorizer(&case.profile, &case.test_file) {
            Ok(_) => println!("✓"),
            Err(e) => panic!("Failed: {}", e),
        }
    }
}

#[test]
#[ignore] // Arista profile not yet implemented
fn test_arista_profiles() {
    println!("\n");
    println!("Testing Arista profiles...");
    let cases: Vec<_> = discover_test_cases()
        .into_iter()
        .filter(|c| c.profile == "arista")
        .collect();

    if cases.is_empty() {
        println!("Skipping: No Arista test cases found in tests/networking/arista/");
        println!("Expected structure: tests/networking/arista/{{device}}/*.conf");
        return;
    }

    for case in cases {
        print!("  {} / {} ... ", case.profile, case.device_type);
        match run_rt_colorizer(&case.profile, &case.test_file) {
            Ok(_) => println!("✓"),
            Err(e) => panic!("Failed: {}", e),
        }
    }
}
