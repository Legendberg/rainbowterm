use std::io::{self, Write};
use std::path::PathBuf;
use regex::Regex;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use clap::{Parser, Subcommand};

mod config;
mod context;
mod matching;
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
            println!("Converted {} to {}", input.display(), output_path.display());
        } else {
            // Write to stdout
            println!("{}", toml_content);
        }

        return Ok(());
    }

    // Reject convert command if feature not enabled
    #[cfg(not(feature = "convert"))]
    if cli.command.is_some() {
        anyhow::bail!(
            "Convert feature is disabled (uses deprecated serde_yaml).\n\
             Enable with: cargo install rainbowterm --features convert"
        );
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

/// Helper function to process and output a single chunk
fn process_and_output_chunk(
    data: &str,
    separator: &str,
    stdout: &mut StandardStream,
    compiled_patterns: &[matching::CompiledPattern],
    context_engine: &mut Option<ContextEngine>,
    config: &Config,
) -> anyhow::Result<()> {
    // Update context state first (before applying patterns)
    if let Some(ref mut engine) = context_engine {
        engine.process_line(data);
    }

    // Collect colored ranges from context rules and patterns
    let mut colored_parts: Vec<(usize, usize, String)> = Vec::new();

    // Context-aware rules (highest priority)
    if let Some(ref engine) = context_engine {
        colored_parts.extend(engine.apply_rules(data, &|c| config.resolve_color(c)));
    }

    // Regular pattern matching (lower priority)
    colored_parts.extend(matching::apply_patterns(data, compiled_patterns));

    // Sort and remove overlaps
    colored_parts.sort_by_key(|k| k.0);
    let final_parts = remove_overlapping_ranges(colored_parts);

    // Render colored output
    render_colored_output(stdout, data, &final_parts)?;
    write!(stdout, "{}", separator)?;

    Ok(())
}

/// Remove overlapping color ranges (keeps first/higher priority)
fn remove_overlapping_ranges(ranges: Vec<(usize, usize, String)>) -> Vec<(usize, usize, String)> {
    let mut result = Vec::new();
    for range in ranges {
        let overlaps = result.iter().any(|(s, e, _)| {
            (range.0 >= *s && range.0 < *e) || (range.1 > *s && range.1 <= *e)
        });
        if !overlaps {
            result.push(range);
        }
    }
    result
}

/// Render text with color ranges to stdout
fn render_colored_output(
    stdout: &mut StandardStream,
    data: &str,
    ranges: &[(usize, usize, String)],
) -> anyhow::Result<()> {
    let mut last_pos = 0;
    for (start, end, color_hex) in ranges {
        write!(stdout, "{}", &data[last_pos..*start])?;
        if let Some((r, g, b)) = parse_hex_color(color_hex) {
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Rgb(r, g, b))))?;
        }
        write!(stdout, "{}", &data[*start..*end])?;
        stdout.reset()?;
        last_pos = *end;
    }
    write!(stdout, "{}", &data[last_pos..])?;
    Ok(())
}

/// Main colorizer with configuration support
fn run_colorizer(
    config: &Config,
    profile: &config::Profile,
    no_color: bool,
    no_context: bool,
) -> anyhow::Result<()> {
    let color_choice = if no_color { ColorChoice::Never } else { ColorChoice::Always };
    let mut stdout = StandardStream::stdout(color_choice);

    // Compile patterns once at startup
    let compiled_patterns = matching::compile_patterns(profile, config);
    let mut context_engine = setup_context_engine(profile, no_context);

    // Process stdin in chunks
    process_stdin(&mut stdout, &compiled_patterns, &mut context_engine, config)
}

/// Setup context engine if enabled
fn setup_context_engine(profile: &config::Profile, no_context: bool) -> Option<ContextEngine> {
    if no_context {
        return None;
    }
    let mut engine = ContextEngine::new();
    for context in &profile.contexts {
        if let Err(e) = engine.add_context(context) {
            eprintln!("Warning: Failed to compile context '{}': {}", context.name, e);
        }
    }
    Some(engine)
}

/// Process stdin in chunks
fn process_stdin(
    stdout: &mut StandardStream,
    patterns: &[matching::CompiledPattern],
    context_engine: &mut Option<ContextEngine>,
    config: &Config,
) -> anyhow::Result<()> {
    use io::Read;

    const READ_SIZE: usize = 8192;
    const BATCH_DELAY_MS: u64 = 10;

    let split_regex = Regex::new(r"(\r\n?|\n)")?;
    let stdin = io::stdin();
    let mut stdin_handle = stdin.lock();
    let mut buffer = Vec::new();

    loop {
        let mut chunk = vec![0u8; READ_SIZE];
        let bytes_read = stdin_handle.read(&mut chunk)?;

        if bytes_read == 0 {
            // EOF - process remaining data
            if !buffer.is_empty() {
                let text = String::from_utf8_lossy(&buffer);
                process_and_output_chunk(&text, "", stdout, patterns, context_engine, config)?;
            }
            break;
        }

        buffer.extend_from_slice(&chunk[..bytes_read]);
        std::thread::sleep(std::time::Duration::from_millis(BATCH_DELAY_MS));

        // Split and process chunks
        let text = String::from_utf8_lossy(&buffer);
        for (data, sep) in split_text_chunks(&text, &split_regex) {
            process_and_output_chunk(&data, &sep, stdout, patterns, context_engine, config)?;
        }

        buffer.clear();
        io::stdout().flush()?;
    }

    Ok(())
}

/// Split text into (data, separator) chunks on line boundaries
fn split_text_chunks(text: &str, regex: &Regex) -> Vec<(String, String)> {
    let mut chunks = Vec::new();
    let mut last_end = 0;
    for mat in regex.find_iter(text) {
        chunks.push((text[last_end..mat.start()].to_string(), mat.as_str().to_string()));
        last_end = mat.end();
    }
    if last_end < text.len() {
        chunks.push((text[last_end..].to_string(), String::new()));
    }
    chunks
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
