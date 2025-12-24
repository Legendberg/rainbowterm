//! Config version registry for smart update detection
//!
//! This module tracks known stock config versions and their hashes to detect
//! whether a user has modified their config from the stock version.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Current embedded config version (must match config.toml header)
pub const CURRENT_VERSION: &str = "0.2.17";

/// Known stock config hashes (version -> blake3 hash)
/// These hashes are computed from the full config.toml content including headers.
/// Add new entries when releasing new versions.
pub static KNOWN_HASHES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // Historical versions - add hash when releasing
    // Format: m.insert("version", "blake3_hash");
    // Note: Config version 0.2.12 was shipped with crates.io package 0.2.13
    m.insert("0.2.12", "eb6f66093568cf23d03c304e49b3e1b054e939a6f2a8610596d652ed9deabe96");
    m.insert("0.2.14", "63bd0ba42f2291a905d0ba6b7df910e25263ef59628cc4a45eca5e8cbdaa3ceb");
    m.insert("0.2.15", "a654bca0bcf2f54daeb422abfb1b621426c72e1cb644ec2f374888f0c316a8ce");
    m.insert("0.2.16", "37326cf0d09c93ffe1bd6f02bee7cf56064a25a57b5f9079fc483287aeb77e1d");
    m.insert("0.2.17", "abf29875ef811b6d24b4fd64eb7634806b45722a7c16b5d689059fded4fa8b10");
    m
});

/// Stock config content by version (for merge operations)
/// We only need to store recent versions for merge capability
#[allow(dead_code)] // Reserved for future three-way merge capability
pub static STOCK_CONFIGS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    // We embed the current config, previous versions would need to be stored
    // For now, we'll only support merge from the immediately previous version
    // which we can reconstruct from git history if needed
    HashMap::new()
});

/// Compute blake3 hash of config content
pub fn hash_config(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

/// Check if a hash matches any known stock version
pub fn find_version_for_hash(hash: &str) -> Option<&'static str> {
    KNOWN_HASHES
        .iter()
        .find(|(_, h)| **h == hash)
        .map(|(v, _)| *v)
}

/// Parse version from config header comment
/// Expects format: "# Config version: X.Y.Z (YYYY-MM-DD)"
pub fn parse_config_version(content: &str) -> Option<String> {
    for line in content.lines().take(10) {
        if line.starts_with("# Config version:") {
            // Extract version part: "0.2.12" from "# Config version: 0.2.12 (2025-12-23)"
            let after_colon = line.split(':').nth(1)?;
            let version = after_colon.trim().split_whitespace().next()?;
            return Some(version.to_string());
        }
    }
    None
}

/// Parse date from config header comment
/// Expects format: "# Config version: X.Y.Z (YYYY-MM-DD)"
pub fn parse_config_date(content: &str) -> Option<String> {
    for line in content.lines().take(10) {
        if line.starts_with("# Config version:") {
            // Extract date part: "2025-12-23" from "# Config version: 0.2.12 (2025-12-23)"
            if let Some(start) = line.find('(') {
                if let Some(end) = line.find(')') {
                    return Some(line[start + 1..end].to_string());
                }
            }
        }
    }
    None
}

/// Check if user's config is a known stock version (unmodified)
pub fn is_stock_config(content: &str) -> Option<&'static str> {
    let hash = hash_config(content);
    find_version_for_hash(&hash)
}

/// Compare two version strings (semver-like comparison)
/// Returns: Less if a < b, Equal if a == b, Greater if a > b
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };

    let va = parse(a);
    let vb = parse(b);

    for (pa, pb) in va.iter().zip(vb.iter()) {
        match pa.cmp(pb) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    va.len().cmp(&vb.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_version() {
        let content = "# RainbowTerm Configuration\n# Context-aware terminal colorizer\n# Config version: 0.2.12 (2025-12-23)\n";
        assert_eq!(parse_config_version(content), Some("0.2.12".to_string()));
    }

    #[test]
    fn test_parse_config_date() {
        let content = "# Config version: 0.2.12 (2025-12-23)\n";
        assert_eq!(parse_config_date(content), Some("2025-12-23".to_string()));
    }

    #[test]
    fn test_hash_config() {
        let hash = hash_config("test content");
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // blake3 produces 64 hex chars
    }

    #[test]
    fn test_compare_versions() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("0.2.12", "0.2.13"), Ordering::Less);
        assert_eq!(compare_versions("0.2.13", "0.2.13"), Ordering::Equal);
        assert_eq!(compare_versions("0.2.15", "0.2.13"), Ordering::Greater);
        assert_eq!(compare_versions("0.3.0", "0.2.99"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "0.9.9"), Ordering::Greater);
    }
}
