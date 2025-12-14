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
                                colored_parts.push(ColoredRange::new(m.start(), m.end(), color.clone()));
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
                                colored_parts.push(ColoredRange::new(m.start(), m.end(), color.clone()));
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
            let caps = regex.captures(input).expect(&format!("Should match: {}", input));
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
}
