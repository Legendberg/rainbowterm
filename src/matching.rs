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
