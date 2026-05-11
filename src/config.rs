use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default fallback color when no color is specified
pub const DEFAULT_COLOR: &str = "#ffffff";

/// A colored text range (start position, end position, hex color)
#[derive(Debug, Clone, PartialEq)]
pub struct ColoredRange {
    pub start: usize,
    pub end: usize,
    pub color: String,
}

impl ColoredRange {
    pub fn new(start: usize, end: usize, color: String) -> Self {
        Self { start, end, color }
    }
}

/// Parse hex color string to RGB tuple
pub fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;

    Some((r, g, b))
}

/// Main configuration structure
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    /// Default profile to use when none is specified
    #[serde(default)]
    pub default_profile: Option<String>,

    /// Color palette definitions
    #[serde(default)]
    pub palette: HashMap<String, String>,

    /// Hostname prefixes for auto-detection (profile_name -> list of prefixes)
    /// Example: { "juniper" = ["jr", "js"], "versa" = ["vr"] }
    #[serde(default)]
    pub hostname_prefixes: HashMap<String, Vec<String>>,

    /// Profile definitions (juniper, cisco, base, etc.)
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,
}

/// A profile represents a device type or vendor configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    /// Human-readable description
    #[serde(default)]
    pub description: String,

    /// Profiles to inherit patterns from
    #[serde(default)]
    pub inherits: Vec<String>,

    /// Auto-detection rules (optional)
    #[serde(default)]
    pub auto_detect: Vec<AutoDetectRule>,

    /// Simple regex patterns
    #[serde(default)]
    pub patterns: Vec<Pattern>,

    /// Context-aware rules (state machine)
    #[serde(default)]
    pub contexts: Vec<Context>,
}

/// Auto-detection rule for profile matching
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutoDetectRule {
    /// Type of detection: "hostname", "prompt", "content"
    pub r#type: String,

    /// Regex pattern to match
    pub pattern: String,
}

/// Simple regex pattern with color
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pattern {
    /// Description of what this pattern matches
    #[serde(default)]
    pub description: String,

    /// Regex pattern
    pub regex: String,

    /// Color (can be palette reference or direct hex)
    #[serde(default)]
    pub color: ColorSpec,

    /// Priority (higher = applied first)
    #[serde(default)]
    pub priority: i32,

    /// Stop matching after this pattern (like ChromaTerm exclusive)
    #[serde(default)]
    pub exclusive: bool,

    /// Case insensitive
    #[serde(default)]
    pub case_insensitive: bool,
}

/// Color specification - can be simple string or group mapping
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ColorSpec {
    /// Simple color: "red" or "#ff0000"
    Simple(String),

    /// Group mapping: { "1": "red", "2": "blue" }
    /// Keys are strings that will be parsed as u32 group numbers
    Groups(HashMap<String, String>),
}

impl Default for ColorSpec {
    fn default() -> Self {
        ColorSpec::Simple(String::new())
    }
}

/// Context-aware rule (state machine)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Context {
    /// Name of this context (e.g., "interface_block")
    pub name: String,

    /// Description
    #[serde(default)]
    pub description: String,

    /// Pattern that starts this context (resets state)
    pub start: String,

    /// State variables to track
    #[serde(default)]
    pub track: Vec<StateTracker>,

    /// Rules that depend on state
    #[serde(default)]
    pub rules: Vec<ContextRule>,
}

/// State variable tracker
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateTracker {
    /// Name of the state variable
    pub name: String,

    /// Regex pattern to extract value
    pub pattern: String,

    /// Which capture group contains the value (default: 1)
    #[serde(default = "default_capture_group")]
    pub capture_group: usize,
}

fn default_capture_group() -> usize {
    1
}

/// Context-aware coloring rule
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContextRule {
    /// Pattern to match
    pub pattern: String,

    /// State variable to check (e.g., "physical_link")
    #[serde(default)]
    pub state_key: Option<String>,

    /// Color mappings based on state value
    #[serde(default)]
    pub colors: Vec<StateColorMapping>,

    /// Default color if no condition matches
    #[serde(default)]
    pub default_color: Option<String>,

    /// Priority
    #[serde(default)]
    pub priority: i32,
}

/// Maps state value to color
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateColorMapping {
    /// State value to match (e.g., "Up")
    pub value: String,

    /// Color to apply when value matches
    pub color: String,
}

impl Config {
    /// Load configuration from TOML file
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Load configuration from string
    pub fn parse(content: &str) -> anyhow::Result<Self> {
        let config: Config = toml::from_str(content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate configuration: profile references and regex compilability.
    ///
    /// Invalid regex is a hard error rather than a warning. Silent
    /// warnings on startup are easy to miss in piped stdout and leave the user
    /// with a subtly broken config (patterns present in TOML but dropped at
    /// compile time). Surfacing the first compile error up-front, including the
    /// underlying regex engine message, is far more actionable.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(ref default_profile) = self.default_profile {
            if !self.profiles.contains_key(default_profile) {
                anyhow::bail!(
                    "default_profile '{}' does not exist in profiles",
                    default_profile
                );
            }
        }

        for (profile_name, profile) in &self.profiles {
            for inherit in &profile.inherits {
                if !self.profiles.contains_key(inherit) {
                    anyhow::bail!(
                        "Profile '{}' inherits from '{}' which does not exist",
                        profile_name,
                        inherit
                    );
                }
            }

            for pattern in &profile.patterns {
                let flags = if pattern.case_insensitive { "(?i)" } else { "" };
                let full_regex = format!("{}{}", flags, pattern.regex);
                if let Err(e) = regex::Regex::new(&full_regex) {
                    anyhow::bail!(
                        "Profile '{}': invalid regex in pattern '{}': {}\n\
                         Pattern: {}\n\
                         Error:   {}",
                        profile_name,
                        pattern.description,
                        pattern.regex,
                        pattern.regex,
                        e
                    );
                }
            }

            for context in &profile.contexts {
                if let Err(e) = regex::Regex::new(&context.start) {
                    anyhow::bail!(
                        "Profile '{}': invalid regex in context '{}' start pattern: {}\n\
                         Pattern: {}\n\
                         Error:   {}",
                        profile_name,
                        context.name,
                        context.start,
                        context.start,
                        e
                    );
                }
                for tracker in &context.track {
                    if let Err(e) = regex::Regex::new(&tracker.pattern) {
                        anyhow::bail!(
                            "Profile '{}': invalid regex in tracker '{}' pattern: {}\n\
                             Pattern: {}\n\
                             Error:   {}",
                            profile_name,
                            tracker.name,
                            tracker.pattern,
                            tracker.pattern,
                            e
                        );
                    }
                }
                for rule in &context.rules {
                    if let Err(e) = regex::Regex::new(&rule.pattern) {
                        anyhow::bail!(
                            "Profile '{}': invalid regex in context rule pattern: {}\n\
                             Pattern: {}\n\
                             Error:   {}",
                            profile_name,
                            rule.pattern,
                            rule.pattern,
                            e
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Get a profile by name, with inheritance resolved (including transitive inheritance)
    pub fn get_profile(&self, name: &str) -> Option<Profile> {
        let mut visited = std::collections::HashSet::new();
        self.resolve_profile_with_inheritance(name, &mut visited)
    }

    /// Helper to resolve profile with cycle detection
    fn resolve_profile_with_inheritance(
        &self,
        name: &str,
        visited: &mut std::collections::HashSet<String>,
    ) -> Option<Profile> {
        // Detect circular inheritance
        if visited.contains(name) {
            eprintln!(
                "Warning: Circular inheritance detected for profile '{}', skipping",
                name
            );
            return None;
        }
        visited.insert(name.to_string());

        let mut profile = self.profiles.get(name)?.clone();

        // Resolve inheritance (including transitive)
        for inherit_name in &profile.inherits.clone() {
            // Recursively resolve parent profile (handles transitive inheritance)
            if let Some(parent) = self.resolve_profile_with_inheritance(inherit_name, visited) {
                // Merge patterns (parent first, then child overrides)
                let mut merged_patterns = parent.patterns.clone();
                merged_patterns.extend(profile.patterns.clone());
                profile.patterns = merged_patterns;

                // Merge contexts
                let mut merged_contexts = parent.contexts.clone();
                merged_contexts.extend(profile.contexts.clone());
                profile.contexts = merged_contexts;
            }
        }

        Some(profile)
    }

    /// Resolve color reference (palette name or hex)
    pub fn resolve_color(&self, color_ref: &str) -> String {
        // If it starts with #, it's already a hex color
        if color_ref.starts_with('#') {
            return color_ref.to_string();
        }

        // Otherwise, look it up in palette
        self.palette
            .get(color_ref)
            .cloned()
            .unwrap_or_else(|| color_ref.to_string())
    }

    /// Auto-detect profile from input content
    /// Returns the profile name and resolved profile if a match is found
    pub fn detect_profile(&self, content: &str) -> Option<(String, Profile)> {
        let mut best_match: Option<(String, Profile, i32)> = None;
        let debug = std::env::var("RT_DEBUG").is_ok();

        if debug {
            eprintln!("DEBUG: Auto-detect content ({} bytes):", content.len());
            eprintln!("DEBUG: Content preview: {:?}", &content[..content.len().min(200)]);
        }

        for (profile_name, profile) in &self.profiles {
            let mut score = 0;

            // Check hostname prefixes first (from [hostname_prefixes] section)
            if let Some(prefixes) = self.hostname_prefixes.get(profile_name) {
                if !prefixes.is_empty() {
                    // Build regex pattern from prefixes: \b(prefix1|prefix2)[0-9]+[a-z0-9\-_]*
                    // Require at least one digit after prefix to avoid matching words like "SWITCH"
                    let prefix_pattern = format!(
                        r"(?i)\b({})[0-9]+[a-z0-9\-_]*",
                        prefixes.iter()
                            .map(|p| regex::escape(p))
                            .collect::<Vec<_>>()
                            .join("|")
                    );
                    if let Ok(regex) = regex::Regex::new(&prefix_pattern) {
                        if regex.is_match(content) {
                            if debug {
                                eprintln!("DEBUG: {} hostname prefix matched: {}", profile_name, prefix_pattern);
                            }
                            score += 50; // hostname match
                        }
                    }
                }
            }

            // Check auto_detect rules from profile
            for rule in &profile.auto_detect {
                // Try to compile the pattern
                let regex = match regex::Regex::new(&rule.pattern) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                match rule.r#type.as_str() {
                    "prompt" => {
                        // Check for CLI prompt patterns (high confidence)
                        if regex.is_match(content) {
                            if debug {
                                eprintln!("DEBUG: {} prompt matched: {}", profile_name, rule.pattern);
                            }
                            score += 100;
                        }
                    }
                    "hostname" => {
                        // Hostname patterns in auto_detect (alternative to [hostname_prefixes])
                        if regex.is_match(content) {
                            if debug {
                                eprintln!("DEBUG: {} hostname matched: {}", profile_name, rule.pattern);
                            }
                            score += 50;
                        }
                    }
                    "content" => {
                        // General content matching (lower confidence)
                        if regex.is_match(content) {
                            if debug {
                                eprintln!("DEBUG: {} content matched: {}", profile_name, rule.pattern);
                            }
                            score += 25;
                        }
                    }
                    _ => {
                        // Unknown type, try general match
                        if regex.is_match(content) {
                            score += 10;
                        }
                    }
                }
            }

            if debug && score > 0 {
                eprintln!("DEBUG: {} total score: {}", profile_name, score);
            }

            // Update best match if this profile scored higher
            if score > 0 {
                if let Some((_, _, best_score)) = &best_match {
                    if score > *best_score {
                        if let Some(resolved) = self.get_profile(profile_name) {
                            best_match = Some((profile_name.clone(), resolved, score));
                        }
                    }
                } else if let Some(resolved) = self.get_profile(profile_name) {
                    best_match = Some((profile_name.clone(), resolved, score));
                }
            }
        }

        best_match.map(|(name, profile, _)| (name, profile))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color_valid() {
        assert_eq!(parse_hex_color("#ff0000"), Some((255, 0, 0)));
        assert_eq!(parse_hex_color("#00ff00"), Some((0, 255, 0)));
        assert_eq!(parse_hex_color("#0000ff"), Some((0, 0, 255)));
        assert_eq!(parse_hex_color("#ffffff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_color("#000000"), Some((0, 0, 0)));
        // Without hash
        assert_eq!(parse_hex_color("ff8c00"), Some((255, 140, 0)));
    }

    #[test]
    fn test_parse_hex_color_invalid() {
        assert_eq!(parse_hex_color("#fff"), None); // Too short
        assert_eq!(parse_hex_color("#fffffff"), None); // Too long
        assert_eq!(parse_hex_color("#gggggg"), None); // Invalid chars
        assert_eq!(parse_hex_color(""), None); // Empty
    }

    #[test]
    fn test_colored_range_new() {
        let range = ColoredRange::new(10, 20, "#ff0000".to_string());
        assert_eq!(range.start, 10);
        assert_eq!(range.end, 20);
        assert_eq!(range.color, "#ff0000");
    }

    #[test]
    fn test_config_parse_minimal() {
        let toml = r##"
            default_profile = "test"
            [palette]
            red = "#ff0000"
            [profiles.test]
            description = "Test profile"
        "##;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.default_profile, Some("test".to_string()));
        assert_eq!(config.palette.get("red"), Some(&"#ff0000".to_string()));
        assert!(config.profiles.contains_key("test"));
    }

    #[test]
    fn test_config_resolve_color_hex() {
        let config = Config::default();
        assert_eq!(config.resolve_color("#ff0000"), "#ff0000");
    }

    #[test]
    fn test_config_resolve_color_palette() {
        let mut config = Config::default();
        config.palette.insert("red".to_string(), "#ff0000".to_string());
        assert_eq!(config.resolve_color("red"), "#ff0000");
    }

    #[test]
    fn test_config_resolve_color_missing() {
        let config = Config::default();
        // Returns the key as-is if not found
        assert_eq!(config.resolve_color("unknown"), "unknown");
    }

    #[test]
    fn test_profile_inheritance() {
        let toml = r##"
            [profiles.base]
            description = "Base profile"
            [[profiles.base.patterns]]
            description = "IP addresses"
            regex = '\d+\.\d+\.\d+\.\d+'
            color = "#00ff00"

            [profiles.child]
            description = "Child profile"
            inherits = ["base"]
            [[profiles.child.patterns]]
            description = "Child pattern"
            regex = 'test'
            color = "#ff0000"
        "##;
        let config = Config::parse(toml).unwrap();
        let profile = config.get_profile("child").unwrap();
        // Should have patterns from both base and child
        assert_eq!(profile.patterns.len(), 2);
    }

    #[test]
    fn test_validate_missing_inherit() {
        let toml = r##"
            [profiles.child]
            description = "Child profile"
            inherits = ["nonexistent"]
        "##;
        let result = Config::parse(toml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("nonexistent"));
    }

    #[test]
    fn test_validate_valid_config() {
        let toml = r##"
            [profiles.test]
            description = "Test"
            [[profiles.test.patterns]]
            regex = '\d+'
            color = "#ff0000"
        "##;
        assert!(Config::parse(toml).is_ok());
    }

    #[test]
    fn test_parse_embedded_config() {
        const CONFIG: &str = include_str!("../config.toml");
        let config = Config::parse(CONFIG).unwrap();

        let juniper_raw = config.profiles.get("juniper").unwrap();
        println!("Direct parse - juniper raw patterns: {}", juniper_raw.patterns.len());

        let second_col: Vec<_> = juniper_raw.patterns.iter()
            .filter(|p| p.description.contains("MAC stats second column"))
            .collect();
        println!("Direct parse - MAC stats second column: {}", second_col.len());
        for p in &second_col {
            println!("  - {} (pri={})", p.description, p.priority);
        }

        assert_eq!(second_col.len(), 5, "Should have 5 MAC stats second column patterns");
    }
}
