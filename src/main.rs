use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;
use regex::Regex;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use clap::{Parser, Subcommand};

mod config;
mod context;
#[cfg(feature = "convert")]
mod convert;

use config::Config;
use context::ContextEngine;

#[derive(Parser)]
#[command(name = "rainbowterm")]
#[command(about = "Context-aware terminal colorizer for network device output", long_about = None)]
#[command(version)]
struct Cli {
    /// Don't use colors
    #[arg(long)]
    no_color: bool,

    /// Configuration file path (default: ~/.rainbowterm.toml)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Profile to use (e.g., juniper, cisco, base)
    #[arg(short, long)]
    profile: Option<String>,

    /// List available profiles and exit
    #[arg(long)]
    list_profiles: bool,

    /// Disable context-aware state machine (pure regex mode)
    #[arg(long)]
    no_context: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert ChromaTerm YAML to RainbowTerm TOML (DEPRECATED - requires 'convert' feature)
    #[cfg(feature = "convert")]
    Convert {
        /// Input YAML file
        input: PathBuf,

        /// Output TOML file (optional, defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handle convert subcommand (feature-gated due to deprecated serde_yaml)
    #[cfg(feature = "convert")]
    if let Some(Commands::Convert { input, output }) = cli.command {
        let yaml_content = std::fs::read_to_string(&input)?;
        let toml_content = convert::convert_yaml_to_toml(&yaml_content)?;

        if let Some(output_path) = output {
            std::fs::write(&output_path, toml_content)?;
            eprintln!("Converted {} to {}", input.display(), output_path.display());
        } else {
            // Write to stdout
            println!("{}", toml_content);
        }

        return Ok(());
    }

    // Reject convert command if feature not enabled
    #[cfg(not(feature = "convert"))]
    if cli.command.is_some() {
        eprintln!("ERROR: Convert feature is disabled (uses deprecated serde_yaml)");
        eprintln!("Enable with: cargo install rainbowterm --features convert");
        std::process::exit(1);
    }

    // Load configuration
    let config_path = cli.config.clone().unwrap_or_else(|| {
        let mut path = dirs::config_dir().expect("Could not find config directory");
        path.push("rainbowterm");
        std::fs::create_dir_all(&path).ok();
        path.push("config.toml");
        path
    });

    // Embedded default config
    const DEFAULT_CONFIG: &str = include_str!("../config.toml");

    // Create config file on first run if it doesn't exist
    if !config_path.exists() && cli.config.is_none() {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
        eprintln!("Created default config at {}", config_path.display());
    }

    // Load config from file or use embedded default
    let config = if config_path.exists() {
        Config::from_file(&config_path)?
    } else {
        Config::from_str(DEFAULT_CONFIG)?
    };

    // Handle --list-profiles
    if cli.list_profiles {
        println!("Available profiles:");
        for (name, profile) in &config.profiles {
            println!("  {} - {}", name, profile.description);
        }
        return Ok(());
    }

    // Get profile name: CLI flag > config default > error
    let profile_name = if let Some(name) = cli.profile.as_ref() {
        name
    } else if let Some(default) = config.default_profile.as_ref() {
        default
    } else {
        anyhow::bail!(
            "No profile specified and no default_profile set in config.\n\
             Use --profile <name> or set default_profile in ~/.rainbowterm.toml"
        );
    };

    let profile = config.get_profile(profile_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Profile '{}' not found. Use --list-profiles to see available profiles.",
            profile_name
        )
    })?;

    // Run the colorizer with the selected profile
    run_colorizer(&config, &profile, cli.no_color, cli.no_context)
}

/// Demo mode with basic hardcoded patterns (no config file)
#[allow(dead_code)]
fn run_demo_mode(no_color: bool) -> anyhow::Result<()> {
    let color_choice = if no_color {
        ColorChoice::Never
    } else {
        ColorChoice::Always
    };

    let mut stdout = StandardStream::stdout(color_choice);
    let stdin = io::stdin();

    // Read raw bytes and handle properly
    use io::Read;
    let mut buffer = Vec::new();
    stdin.lock().read_to_end(&mut buffer)?;

    // Split on newlines and carriage returns, keeping separators
    let split_regex = Regex::new(r"(\r\n?|\n)")?;
    let text = String::from_utf8_lossy(&buffer);

    let mut chunks: Vec<(String, String)> = Vec::new();
    let mut last_end = 0;

    for mat in split_regex.find_iter(&text) {
        let data = text[last_end..mat.start()].to_string();
        let separator = text[mat.start()..mat.end()].to_string();
        chunks.push((data, separator));
        last_end = mat.end();
    }

    if last_end < text.len() {
        chunks.push((text[last_end..].to_string(), String::new()));
    }

    let interface_regex = Regex::new(r"\b((?:ge|xe|et|fe|mge|pfe)-?\d+(?:/\d+)*(?:\.\d+)?)\b")?;
    let up_regex = Regex::new(r"\b(up|Up|UP)\b")?;
    let down_regex = Regex::new(r"\b(down|Down|DOWN)\b")?;

    for (data, separator) in chunks {
        let mut colored_parts: Vec<(usize, usize, Color)> = Vec::new();

        for cap in interface_regex.captures_iter(&data) {
            if let Some(m) = cap.get(1) {
                colored_parts.push((m.start(), m.end(), Color::Cyan));
            }
        }

        for cap in up_regex.captures_iter(&data) {
            if let Some(m) = cap.get(1) {
                colored_parts.push((m.start(), m.end(), Color::Green));
            }
        }

        for cap in down_regex.captures_iter(&data) {
            if let Some(m) = cap.get(1) {
                colored_parts.push((m.start(), m.end(), Color::Red));
            }
        }

        colored_parts.sort_by_key(|k| k.0);

        let mut last_pos = 0;
        for (start, end, color) in colored_parts {
            write!(stdout, "{}", &data[last_pos..start])?;
            stdout.set_color(ColorSpec::new().set_fg(Some(color)))?;
            write!(stdout, "{}", &data[start..end])?;
            stdout.reset()?;
            last_pos = end;
        }

        // Write remaining data and separator unchanged
        write!(stdout, "{}", &data[last_pos..])?;
        write!(stdout, "{}", separator)?;
    }

    Ok(())
}

/// Color specification for pattern matching (resolved from config)
#[derive(Debug, Clone)]
enum ResolvedColorSpec {
    Simple(String),
    Groups(std::collections::HashMap<u32, String>),
}

/// Helper function to process and output a single chunk
fn process_and_output_chunk(
    data: &str,
    separator: &str,
    stdout: &mut StandardStream,
    compiled_patterns: &[(Regex, ResolvedColorSpec, i32, bool)],
    context_engine: &mut Option<ContextEngine>,
    config: &Config,
) -> anyhow::Result<()> {
    let mut colored_parts: Vec<(usize, usize, String)> = Vec::new();

    // Update context state first (before applying patterns)
    if let Some(ref mut engine) = context_engine {
        engine.process_line(data);
    }

    // Apply context-aware rules (highest priority)
    if let Some(ref engine) = context_engine {
        let palette_resolver = |color_ref: &str| config.resolve_color(color_ref);
        let context_colors = engine.apply_rules(data, &palette_resolver);
        colored_parts.extend(context_colors);
    }

    // Apply regular patterns (lower priority)
    // Note: exclusive flag only stops finding more instances of the SAME pattern,
    // not all patterns. Overlap removal happens later based on priority.
    for (regex, color_spec, _priority, exclusive) in compiled_patterns {
        for cap in regex.captures_iter(data) {
            // Check if there are capture groups (beyond group 0)
            if cap.len() > 1 {
                // Color only the captured groups, not the whole match
                match color_spec {
                    ResolvedColorSpec::Simple(color) => {
                        // Apply same color to all capture groups
                        for i in 1..cap.len() {
                            if let Some(m) = cap.get(i) {
                                colored_parts.push((m.start(), m.end(), color.clone()));
                            }
                        }
                    }
                    ResolvedColorSpec::Groups(group_colors) => {
                        // Apply different colors to each capture group
                        for i in 1..cap.len() {
                            if let Some(m) = cap.get(i) {
                                // Look up color for this specific group number
                                if let Some(color) = group_colors.get(&(i as u32)) {
                                    colored_parts.push((m.start(), m.end(), color.clone()));
                                }
                                // If no color specified for this group, don't color it
                            }
                        }
                    }
                }
            } else if let Some(m) = cap.get(0) {
                // No capture groups, color the whole match with simple color only
                if let ResolvedColorSpec::Simple(color) = color_spec {
                    colored_parts.push((m.start(), m.end(), color.clone()));
                }
            }

            if *exclusive {
                // Stop looking for more instances of THIS pattern only
                break;
            }
        }
    }

    // Sort by position and remove overlaps
    colored_parts.sort_by_key(|k| k.0);

    let mut final_parts: Vec<(usize, usize, String)> = Vec::new();
    for part in colored_parts {
        let overlaps = final_parts.iter().any(|(s, e, _)| {
            (part.0 >= *s && part.0 < *e) || (part.1 > *s && part.1 <= *e)
        });
        if !overlaps {
            final_parts.push(part);
        }
    }

    // Print the data with colors
    let mut last_pos = 0;
    for (start, end, color_hex) in final_parts {
        write!(stdout, "{}", &data[last_pos..start])?;

        // Convert hex to RGB
        if let Some(rgb) = parse_hex_color(&color_hex) {
            let mut spec = ColorSpec::new();
            spec.set_fg(Some(Color::Rgb(rgb.0, rgb.1, rgb.2)));
            stdout.set_color(&spec)?;
        }

        write!(stdout, "{}", &data[start..end])?;
        stdout.reset()?;

        last_pos = end;
    }

    // Write remaining data and separator unchanged (like ChromaTerm)
    write!(stdout, "{}", &data[last_pos..])?;
    write!(stdout, "{}", separator)?;

    Ok(())
}

/// Main colorizer with configuration support
fn run_colorizer(
    config: &Config,
    profile: &config::Profile,
    no_color: bool,
    no_context: bool,
) -> anyhow::Result<()> {
    let color_choice = if no_color {
        ColorChoice::Never
    } else {
        ColorChoice::Always
    };

    let mut stdout = StandardStream::stdout(color_choice);
    let stdin = io::stdin();

    // Read in chunks like ChromaTerm does (8192 bytes at a time)
    use io::Read;
    use std::thread;
    const READ_SIZE: usize = 8192;
    const BATCH_DELAY_MS: u64 = 10; // Small delay to batch rapid input

    let split_regex = Regex::new(r"(\r\n?|\n)")?;

    let mut stdin_handle = stdin.lock();
    let mut accumulated_buffer = Vec::new();

    // Compile all patterns from profile
    let mut compiled_patterns: Vec<(Regex, ResolvedColorSpec, i32, bool)> = Vec::new();

    for pattern in &profile.patterns {
        let regex_str = &pattern.regex;
        let flags = if pattern.case_insensitive { "(?i)" } else { "" };
        let full_regex = format!("{}{}", flags, regex_str);

        match Regex::new(&full_regex) {
            Ok(regex) => {
                // Resolve color from palette
                let resolved_color = match &pattern.color {
                    config::ColorSpec::Simple(c) => {
                        ResolvedColorSpec::Simple(config.resolve_color(c))
                    }
                    config::ColorSpec::Groups(groups) => {
                        // Resolve each group color through the palette
                        // Parse string keys as u32 group numbers
                        let mut resolved_groups = std::collections::HashMap::new();
                        for (group_str, color_ref) in groups {
                            if let Ok(group_num) = group_str.parse::<u32>() {
                                resolved_groups.insert(group_num, config.resolve_color(color_ref));
                            } else {
                                eprintln!("Warning: Invalid group number '{}' in pattern '{}'", group_str, pattern.description);
                            }
                        }
                        ResolvedColorSpec::Groups(resolved_groups)
                    }
                };
                compiled_patterns.push((regex, resolved_color, pattern.priority, pattern.exclusive));
            }
            Err(e) => {
                eprintln!("Warning: Failed to compile pattern '{}': {}", pattern.description, e);
            }
        }
    }

    // Sort patterns by priority (highest first)
    compiled_patterns.sort_by(|a, b| b.2.cmp(&a.2));

    // Initialize context engine if context-aware mode is enabled
    let mut context_engine = if !no_context {
        let mut engine = ContextEngine::new();
        for context in &profile.contexts {
            if let Err(e) = engine.add_context(context) {
                eprintln!("Warning: Failed to compile context '{}': {}", context.name, e);
            }
        }
        Some(engine)
    } else {
        None
    };

    // Read and process chunks in a loop - simple approach
    loop {
        let mut chunk_buffer = vec![0u8; READ_SIZE];
        let bytes_read = stdin_handle.read(&mut chunk_buffer)?;

        if bytes_read == 0 {
            // EOF reached - process any remaining data
            if !accumulated_buffer.is_empty() {
                let text = String::from_utf8_lossy(&accumulated_buffer);
                process_and_output_chunk(
                    &text,
                    "",
                    &mut stdout,
                    &compiled_patterns,
                    &mut context_engine,
                    config,
                )?;
            }
            break;
        }

        // Append new data to accumulated buffer
        accumulated_buffer.extend_from_slice(&chunk_buffer[..bytes_read]);

        // Small delay to batch rapid input (like typing)
        thread::sleep(Duration::from_millis(BATCH_DELAY_MS));

        // Split buffer into (data, separator) tuples
        let text = String::from_utf8_lossy(&accumulated_buffer);
        let mut chunks: Vec<(String, String)> = Vec::new();
        let mut last_end = 0;

        for mat in split_regex.find_iter(&text) {
            let data = text[last_end..mat.start()].to_string();
            let separator = text[mat.start()..mat.end()].to_string();
            chunks.push((data, separator));
            last_end = mat.end();
        }

        // If there's incomplete data (like a prompt), output it too
        if last_end < text.len() {
            chunks.push((text[last_end..].to_string(), String::new()));
        }

        // Process all chunks
        for (data, separator) in chunks {
            process_and_output_chunk(
                &data,
                &separator,
                &mut stdout,
                &compiled_patterns,
                &mut context_engine,
                config,
            )?;
        }

        // Clear buffer after processing
        accumulated_buffer.clear();

        // Flush output
        io::stdout().flush()?;
    }

    Ok(())
}

/// Parse hex color string to RGB tuple
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;

    Some((r, g, b))
}
