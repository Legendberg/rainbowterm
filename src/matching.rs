//! Pattern matching module for RainbowTerm
//!
//! Handles regex compilation with palette resolution and pattern application.
//! Extracted from main.rs to improve modularity and testability.

use regex::Regex;
use std::collections::HashMap;

use crate::config::{self, ColoredRange, Config, Profile};

/// Resolved color specification (post-palette lookup)
#[derive(Debug, Clone)]
pub enum ResolvedColorSpec {
    Simple(String),
    Groups(HashMap<u32, String>),
}

/// Compiled pattern with resolved colors: (regex, color_spec, priority, exclusive)
pub type CompiledPattern = (Regex, ResolvedColorSpec, i32, bool);

/// Compile all patterns from profile with palette resolution
pub fn compile_patterns(profile: &Profile, config: &Config) -> Vec<CompiledPattern> {
    let mut compiled = Vec::new();

    for pattern in &profile.patterns {
        let flags = if pattern.case_insensitive { "(?i)" } else { "" };
        let full_regex = format!("{}{}", flags, pattern.regex);

        match Regex::new(&full_regex) {
            Ok(regex) => {
                let resolved_color = resolve_color_spec(&pattern.color, config);
                compiled.push((regex, resolved_color, pattern.priority, pattern.exclusive));
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to compile pattern '{}': {}",
                    pattern.description, e
                );
            }
        }
    }

    // Sort by priority (highest first)
    compiled.sort_by(|a, b| b.2.cmp(&a.2));
    compiled
}

/// Resolve a color specification through the palette
fn resolve_color_spec(color: &config::ColorSpec, config: &Config) -> ResolvedColorSpec {
    match color {
        config::ColorSpec::Simple(c) => ResolvedColorSpec::Simple(config.resolve_color(c)),
        config::ColorSpec::Groups(groups) => {
            let mut resolved = HashMap::new();
            for (group_str, color_ref) in groups {
                if let Ok(num) = group_str.parse::<u32>() {
                    resolved.insert(num, config.resolve_color(color_ref));
                }
            }
            ResolvedColorSpec::Groups(resolved)
        }
    }
}

/// Apply compiled patterns to text and return colored ranges
pub fn apply_patterns(data: &str, patterns: &[CompiledPattern]) -> Vec<ColoredRange> {
    let mut colored_parts = Vec::new();

    for (regex, color_spec, _priority, exclusive) in patterns {
        for cap in regex.captures_iter(data) {
            match color_spec {
                ResolvedColorSpec::Simple(color) => {
                    if cap.len() > 1 {
                        // Color capture groups
                        for i in 1..cap.len() {
                            if let Some(m) = cap.get(i) {
                                colored_parts.push(ColoredRange::new(
                                    m.start(),
                                    m.end(),
                                    color.clone(),
                                ));
                            }
                        }
                    } else if let Some(m) = cap.get(0) {
                        // Color whole match
                        colored_parts.push(ColoredRange::new(m.start(), m.end(), color.clone()));
                    }
                }
                ResolvedColorSpec::Groups(group_colors) => {
                    for i in 1..cap.len() {
                        if let Some(m) = cap.get(i) {
                            if let Some(color) = group_colors.get(&(i as u32)) {
                                colored_parts.push(ColoredRange::new(
                                    m.start(),
                                    m.end(),
                                    color.clone(),
                                ));
                            }
                        }
                    }
                }
            }

            if *exclusive {
                break; // Stop looking for more instances of THIS pattern
            }
        }
    }

    colored_parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn make_simple_pattern(regex: &str, color: &str, priority: i32) -> CompiledPattern {
        (
            Regex::new(regex).unwrap(),
            ResolvedColorSpec::Simple(color.to_string()),
            priority,
            false,
        )
    }

    #[test]
    fn test_apply_patterns_simple_match() {
        let patterns = vec![make_simple_pattern(r"\d+\.\d+\.\d+\.\d+", "#00ff00", 100)];
        let result = apply_patterns("IP: 192.168.1.1", &patterns);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start, 4);
        assert_eq!(result[0].end, 15);
        assert_eq!(result[0].color, "#00ff00");
    }

    #[test]
    fn test_apply_patterns_multiple_matches() {
        let patterns = vec![make_simple_pattern(r"\d+\.\d+\.\d+\.\d+", "#00ff00", 100)];
        let result = apply_patterns("From 10.0.0.1 to 192.168.1.1", &patterns);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_apply_patterns_no_match() {
        let patterns = vec![make_simple_pattern(r"\d+\.\d+\.\d+\.\d+", "#00ff00", 100)];
        let result = apply_patterns("No IP here", &patterns);
        assert!(result.is_empty());
    }

    #[test]
    fn test_apply_patterns_capture_group() {
        // Pattern with capture group - should only color the group
        let patterns = vec![make_simple_pattern(r"Status: (\w+)", "#ff0000", 100)];
        let result = apply_patterns("Status: Up", &patterns);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].start, 8); // "Up" starts at position 8
        assert_eq!(result[0].end, 10);
    }

    #[test]
    fn test_apply_patterns_exclusive() {
        let patterns = vec![(
            Regex::new(r"\d+").unwrap(),
            ResolvedColorSpec::Simple("#ff0000".to_string()),
            100,
            true, // exclusive
        )];
        let result = apply_patterns("123 456 789", &patterns);
        // Exclusive should stop after first match
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_apply_patterns_group_colors() {
        let mut group_colors = HashMap::new();
        group_colors.insert(1, "#ff0000".to_string());
        group_colors.insert(2, "#00ff00".to_string());

        let patterns = vec![(
            Regex::new(r"(\w+)@(\w+)").unwrap(),
            ResolvedColorSpec::Groups(group_colors),
            100,
            false,
        )];
        let result = apply_patterns("user@host", &patterns);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].color, "#ff0000"); // "user"
        assert_eq!(result[1].color, "#00ff00"); // "host"
    }

    #[test]
    fn test_alternation_capture() {
        // Test that alternation inside capture groups works correctly
        let pattern = r"(?i)(Input errors|Output errors|Errors|Drops|CRC errors):\s+(0)\b";
        let regex = Regex::new(pattern).unwrap();

        let text = "Input errors: 0";
        let caps = regex.captures(text).unwrap();

        assert!(caps.get(1).is_some(), "Group 1 should match");
        assert!(caps.get(2).is_some(), "Group 2 should match");
        assert_eq!(caps.get(1).unwrap().as_str(), "Input errors");
        assert_eq!(caps.get(2).unwrap().as_str(), "0");

        let text2 = "Drops: 0";
        let caps2 = regex.captures(text2).unwrap();
        assert_eq!(caps2.get(1).unwrap().as_str(), "Drops");
        assert_eq!(caps2.get(2).unwrap().as_str(), "0");
    }

    #[test]
    fn test_alternation_capture_all_variants() {
        // Test all alternation options to ensure they all capture correctly
        let pattern = r"(?i)(Input errors|Output errors|Errors|Drops|Framing errors|Runts|Giants|Collisions|CRC errors):\s+(0)\b";
        let regex = Regex::new(pattern).unwrap();

        let test_cases = vec![
            ("Input errors: 0", "Input errors"),
            ("Output errors: 0", "Output errors"),
            ("Errors: 0", "Errors"),
            ("Drops: 0", "Drops"),
            ("Framing errors: 0", "Framing errors"),
            ("Runts: 0", "Runts"),
            ("Giants: 0", "Giants"),
            ("Collisions: 0", "Collisions"),
            ("CRC errors: 0", "CRC errors"),
        ];

        for (input, expected_label) in test_cases {
            let caps = regex
                .captures(input)
                .expect(&format!("Should match: {}", input));
            assert_eq!(
                caps.get(1).unwrap().as_str(),
                expected_label,
                "Label should match for: {}",
                input
            );
            assert_eq!(
                caps.get(2).unwrap().as_str(),
                "0",
                "Number should be 0 for: {}",
                input
            );
        }
    }

    #[test]
    fn test_apply_patterns_alternation_groups() {
        // Test apply_patterns with alternation inside capture groups
        let mut group_colors = HashMap::new();
        group_colors.insert(1, "#888888".to_string()); // gray for label
        group_colors.insert(2, "#00ff00".to_string()); // green for 0

        let patterns = vec![(
            Regex::new(r"(?i)(Input errors|Output errors|Errors|Drops):\s+(0)\b").unwrap(),
            ResolvedColorSpec::Groups(group_colors),
            175,
            false,
        )];

        let result = apply_patterns("Input errors: 0", &patterns);
        assert_eq!(result.len(), 2, "Should have 2 colored ranges");

        // First range should be "Input errors" (gray)
        assert_eq!(result[0].start, 0);
        assert_eq!(result[0].end, 12);
        assert_eq!(result[0].color, "#888888");

        // Second range should be "0" (green)
        assert_eq!(result[1].start, 14);
        assert_eq!(result[1].end, 15);
        assert_eq!(result[1].color, "#00ff00");
    }

    #[test]
    fn test_apply_patterns_multiple_patterns_same_text() {
        // Test with TWO patterns matching same text - simulates the actual config
        let mut group_colors = HashMap::new();
        group_colors.insert(1, "#888888".to_string()); // gray for label
        group_colors.insert(2, "#00ff00".to_string()); // green for 0

        // Pattern 1: semantic pattern with capturing groups (priority 175)
        let semantic = (
            Regex::new(r"(?i)(Input errors|Output errors|Errors|Drops):\s+(0)\b").unwrap(),
            ResolvedColorSpec::Groups(group_colors),
            175,
            false,
        );

        // Pattern 2: older pattern with non-capturing group (priority 168)
        let older = (
            Regex::new(r"(?i)(?:Input errors|Output errors|Errors|Drops)\s*:\s+(0)\b").unwrap(),
            ResolvedColorSpec::Simple("#00ff00".to_string()),
            168,
            false,
        );

        // Patterns sorted by priority (highest first)
        let patterns = vec![semantic, older];

        let result = apply_patterns("Input errors: 0", &patterns);

        // Should have 3 ranges: (0,12,gray), (14,15,green), (14,15,green)
        assert_eq!(result.len(), 3, "Should have 3 colored ranges before dedup");

        // Verify first range is "Input errors" (gray)
        let input_errors_range = result.iter().find(|r| r.start == 0).unwrap();
        assert_eq!(input_errors_range.end, 12);
        assert_eq!(input_errors_range.color, "#888888");
    }

    #[test]
    fn test_loop_detect_pdu_error_pattern() {
        // This is the exact pattern from config.toml for "Error: None (healthy)"
        let pattern = r"(?i)((?:BPDU|Loop Detect PDU|Ethernet-Switching|MAC-REWRITE|[\w-]+)\s+Error):\s+(None)\b";
        let regex = Regex::new(pattern).unwrap();

        // Test individual cases
        let test_cases = vec![
            ("BPDU Error: None", "BPDU Error"),
            ("Loop Detect PDU Error: None", "Loop Detect PDU Error"),
            ("Ethernet-Switching Error: None", "Ethernet-Switching Error"),
            ("MAC-REWRITE Error: None", "MAC-REWRITE Error"),
            ("CRC Error: None", "CRC Error"),
        ];

        for (input, expected_group1) in test_cases {
            let caps = regex
                .captures(input)
                .expect(&format!("Should match: {}", input));
            let actual = caps.get(1).unwrap().as_str();
            assert_eq!(
                actual, expected_group1,
                "For input '{}': expected group 1 '{}', got '{}'",
                input, expected_group1, actual
            );
        }

        // Test comma-separated line (real-world case)
        let line = "BPDU Error: None, Loop Detect PDU Error: None, Ethernet-Switching Error: None";
        let matches: Vec<_> = regex
            .captures_iter(line)
            .map(|c| c.get(1).unwrap().as_str().to_string())
            .collect();

        assert_eq!(matches.len(), 3, "Should find 3 matches");
        assert_eq!(matches[0], "BPDU Error");
        assert_eq!(matches[1], "Loop Detect PDU Error");
        assert_eq!(matches[2], "Ethernet-Switching Error");
    }

    #[test]
    fn test_loop_detect_pdu_with_full_config() {
        use std::path::Path;

        // Load the actual config
        let config = Config::from_file(Path::new("config.toml")).unwrap();
        let profile = config.get_profile("juniper").unwrap();
        let patterns = compile_patterns(&profile, &config);

        let line = "BPDU Error: None, Loop Detect PDU Error: None, Ethernet-Switching Error: None";

        // Find the "Error: None (healthy)" pattern
        let error_none_pattern = patterns
            .iter()
            .find(|(regex, _, priority, _)| *priority == 205 && regex.as_str().contains("Error"));

        assert!(
            error_none_pattern.is_some(),
            "Should find Error: None pattern with priority 205"
        );

        let (regex, _, priority, _) = error_none_pattern.unwrap();
        println!("Found pattern at priority {}: {}", priority, regex.as_str());

        // Test that the regex matches correctly
        let matches: Vec<_> = regex.captures_iter(line).collect();
        assert_eq!(
            matches.len(),
            3,
            "Should find 3 matches for Error: None pattern"
        );

        // Check second match is "Loop Detect PDU Error"
        let group1 = matches[1].get(1).unwrap().as_str();
        assert_eq!(
            group1, "Loop Detect PDU Error",
            "Second match group 1 should be 'Loop Detect PDU Error', got '{}'",
            group1
        );
    }

    #[test]
    fn test_loop_detect_pdu_apply_patterns() {
        use std::path::Path;

        // Load the actual config
        let config = Config::from_file(Path::new("config.toml")).unwrap();
        let profile = config.get_profile("juniper").unwrap();
        let patterns = compile_patterns(&profile, &config);

        let line = "BPDU Error: None, Loop Detect PDU Error: None";

        // Apply all patterns
        let ranges = apply_patterns(line, &patterns);

        // Find all ranges that touch "Loop Detect PDU Error"
        // "Loop Detect PDU Error" is at positions 18-39 in the line
        let loop_detect_start = line.find("Loop").unwrap();
        let loop_detect_end =
            line.find("Loop Detect PDU Error").unwrap() + "Loop Detect PDU Error".len();

        println!("Line: {}", line);
        println!(
            "Loop Detect PDU Error at {}-{}",
            loop_detect_start, loop_detect_end
        );
        println!("\nAll colored ranges ({} total):", ranges.len());

        for range in &ranges {
            let text = &line[range.start..range.end];
            println!(
                "  {}-{}: '{}' [{}]",
                range.start, range.end, text, range.color
            );
        }

        // Find the range for "Loop Detect PDU Error"
        let loop_detect_range = ranges
            .iter()
            .find(|r| r.start == loop_detect_start && r.end == loop_detect_end);

        assert!(
            loop_detect_range.is_some(),
            "Should find a colored range for 'Loop Detect PDU Error' at {}-{}",
            loop_detect_start,
            loop_detect_end
        );

        // Now test with overlap removal (same logic as main.rs)
        let mut sorted_ranges = ranges;
        sorted_ranges.sort_by_key(|k| k.start);

        // Remove overlapping ranges
        let mut final_ranges = Vec::new();
        for range in sorted_ranges {
            let overlaps = final_ranges.iter().any(|r: &ColoredRange| {
                (range.start >= r.start && range.start < r.end)
                    || (range.end > r.start && range.end <= r.end)
            });
            if !overlaps {
                final_ranges.push(range);
            }
        }

        println!("\nAfter overlap removal ({} ranges):", final_ranges.len());
        for range in &final_ranges {
            let text = &line[range.start..range.end];
            println!(
                "  {}-{}: '{}' [{}]",
                range.start, range.end, text, range.color
            );
        }

        // Verify "Loop Detect PDU Error" survives overlap removal
        let final_loop_detect = final_ranges
            .iter()
            .find(|r| r.start == loop_detect_start && r.end == loop_detect_end);

        assert!(
            final_loop_detect.is_some(),
            "Should have 'Loop Detect PDU Error' range after overlap removal at {}-{}",
            loop_detect_start,
            loop_detect_end
        );
    }

    #[test]
    fn test_compile_patterns_sorts_by_priority() {
        let toml = r##"
            [profiles.test]
            description = "Test"
            [[profiles.test.patterns]]
            regex = 'low'
            color = "#111111"
            priority = 10
            [[profiles.test.patterns]]
            regex = 'high'
            color = "#222222"
            priority = 100
            [[profiles.test.patterns]]
            regex = 'medium'
            color = "#333333"
            priority = 50
        "##;
        let config = Config::parse(toml).unwrap();
        let profile = config.get_profile("test").unwrap();
        let compiled = compile_patterns(&profile, &config);

        assert_eq!(compiled.len(), 3);
        assert_eq!(compiled[0].2, 100); // highest priority first
        assert_eq!(compiled[1].2, 50);
        assert_eq!(compiled[2].2, 10);
    }

    #[test]
    fn test_count_juniper_patterns() {
        use std::path::Path;

        // Load the actual config
        let config = Config::from_file(Path::new("config.toml")).unwrap();
        let profile = config.get_profile("juniper").unwrap();

        println!(
            "\nJuniper profile has {} patterns total",
            profile.patterns.len()
        );

        // Find MAC stats second column patterns
        let second_col: Vec<_> = profile
            .patterns
            .iter()
            .filter(|p| p.description.contains("MAC stats second column"))
            .collect();

        println!("MAC stats second column patterns: {}", second_col.len());
        for p in &second_col {
            println!("  - {} (pri={})", p.description, p.priority);
        }

        // Print last 15 patterns to see where we stopped
        println!("\nLast 15 patterns:");
        let start = if profile.patterns.len() > 15 {
            profile.patterns.len() - 15
        } else {
            0
        };
        for (i, p) in profile.patterns.iter().skip(start).enumerate() {
            println!(
                "  {}. {} (pri={})",
                start + i + 1,
                p.description,
                p.priority
            );
        }

        assert!(
            second_col.len() >= 5,
            "Should have at least 5 MAC stats second column patterns"
        );
    }
}
