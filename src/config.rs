use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main configuration structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Default profile to use when none is specified
    #[serde(default)]
    pub default_profile: Option<String>,

    /// Color palette definitions
    #[serde(default)]
    pub palette: HashMap<String, String>,

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
        Ok(config)
    }

    /// Load configuration from string
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let config: Config = toml::from_str(content)?;
        Ok(config)
    }

    /// Get a profile by name, with inheritance resolved
    pub fn get_profile(&self, name: &str) -> Option<Profile> {
        let mut profile = self.profiles.get(name)?.clone();

        // Resolve inheritance
        for inherit_name in &profile.inherits.clone() {
            if let Some(parent) = self.profiles.get(inherit_name) {
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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_profile: None,
            palette: HashMap::new(),
            profiles: HashMap::new(),
        }
    }
}
