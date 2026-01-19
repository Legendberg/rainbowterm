// This module is conditionally compiled via #[cfg(feature = "convert")] in lib.rs and main.rs

use serde::Deserialize;
use std::collections::HashMap;

use crate::config::{Config, Pattern, Profile, ColorSpec};

/// ChromaTerm YAML structure (simplified)
#[derive(Debug, Deserialize)]
pub struct ChromaTermConfig {
    #[serde(default)]
    pub palette: HashMap<String, String>,

    #[serde(default)]
    pub rules: Vec<ChromaTermRule>,
}

#[derive(Debug, Deserialize)]
pub struct ChromaTermRule {
    #[serde(default)]
    pub description: String,

    pub regex: String,

    #[serde(default)]
    pub color: ChromaTermColor,

    #[serde(default)]
    pub exclusive: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ChromaTermColor {
    Simple(String),
    // Handle both numeric and string keys
    Mixed(serde_yaml::Value),
}

impl Default for ChromaTermColor {
    fn default() -> Self {
        ChromaTermColor::Simple(String::new())
    }
}

/// Convert ChromaTerm YAML to RainbowTerm TOML
pub fn convert_yaml_to_toml(yaml_content: &str) -> anyhow::Result<String> {
    // Parse YAML
    let ct_config: ChromaTermConfig = serde_yaml::from_str(yaml_content)?;

    // Create RainbowTerm config with palette from ChromaTerm
    let mut rt_config = Config {
        palette: ct_config.palette.clone(),
        ..Default::default()
    };

    // Create a profile from the rules
    let mut profile = Profile {
        description: "Converted from ChromaTerm YAML".to_string(),
        inherits: vec![],
        auto_detect: vec![],
        patterns: vec![],
        contexts: vec![],
    };

    // Convert rules to patterns
    for (priority, rule) in ct_config.rules.iter().enumerate() {
        let pattern = Pattern {
            description: rule.description.clone(),
            regex: rule.regex.clone(),
            color: convert_color(&rule.color),
            priority: -(priority as i32), // Negative to maintain order
            exclusive: rule.exclusive,
            case_insensitive: rule.regex.starts_with("(?i)"),
        };

        profile.patterns.push(pattern);
    }

    // Add profile to config
    rt_config.profiles.insert("converted".to_string(), profile);

    // Serialize to TOML
    let toml_string = toml::to_string_pretty(&rt_config)?;

    Ok(toml_string)
}

fn convert_color(ct_color: &ChromaTermColor) -> ColorSpec {
    match ct_color {
        ChromaTermColor::Simple(s) => {
            // Handle f. and b. prefixes from ChromaTerm
            let cleaned = s.trim_start_matches("f.").trim_start_matches("b.").trim();
            ColorSpec::Simple(cleaned.to_string())
        }
        ChromaTermColor::Mixed(value) => {
            // Try to extract colors from the mixed value
            if let Some(s) = value.as_str() {
                // It's a simple string
                let cleaned = s.trim_start_matches("f.").trim_start_matches("b.").trim();
                ColorSpec::Simple(cleaned.to_string())
            } else if let Some(map) = value.as_mapping() {
                // It's a map - could be numeric or string keys
                let mut converted_groups = HashMap::new();

                for (k, v) in map {
                    if let (Some(key_num), Some(val_str)) = (k.as_u64(), v.as_str()) {
                        // Numeric key (capture group) - convert to string
                        let cleaned = val_str.trim_start_matches("f.").trim_start_matches("b.").trim();
                        converted_groups.insert(key_num.to_string(), cleaned.to_string());
                    } else if let (Some(key_str), Some(val_str)) = (k.as_str(), v.as_str()) {
                        // String key - use first one as simple color
                        if key_str == "0" || converted_groups.is_empty() {
                            let cleaned = val_str.trim_start_matches("f.").trim_start_matches("b.").trim();
                            return ColorSpec::Simple(cleaned.to_string());
                        }
                    }
                }

                if !converted_groups.is_empty() {
                    ColorSpec::Groups(converted_groups)
                } else {
                    ColorSpec::Simple(String::new())
                }
            } else {
                ColorSpec::Simple(String::new())
            }
        }
    }
}
