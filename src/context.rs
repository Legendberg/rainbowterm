use anyhow::{Context as AnyhowContext, Result};
use regex::Regex;
use std::collections::HashMap;

use crate::config::{ColoredRange, Context, DEFAULT_COLOR};

/// Compiled context with pre-compiled regex patterns
pub struct CompiledContext {
    pub name: String,
    pub start_regex: Regex,
    pub trackers: Vec<CompiledTracker>,
    pub rules: Vec<CompiledContextRule>,
}

/// Compiled state tracker
pub struct CompiledTracker {
    pub name: String,
    pub regex: Regex,
    pub capture_group: usize,
}

/// Compiled context rule
pub struct CompiledContextRule {
    pub regex: Regex,
    pub state_key: Option<String>,
    pub color_mappings: HashMap<String, String>,
    pub default_color: Option<String>,
    pub priority: i32,
}

/// Runtime state for a single context instance
#[derive(Debug, Clone)]
pub struct ContextState {
    pub variables: HashMap<String, String>,
}

impl Default for ContextState {
    fn default() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }
}

impl ContextState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.variables.clear();
    }

    pub fn set(&mut self, key: String, value: String) {
        self.variables.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.variables.get(key)
    }
}

/// Context engine manages multiple contexts
pub struct ContextEngine {
    contexts: Vec<CompiledContext>,
    states: HashMap<String, ContextState>,
}

impl Default for ContextEngine {
    fn default() -> Self {
        Self {
            contexts: Vec::new(),
            states: HashMap::new(),
        }
    }
}

impl ContextEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compile and add a context from config
    pub fn add_context(&mut self, context: &Context) -> Result<()> {
        let start_regex = Regex::new(&context.start)
            .with_context(|| format!("Failed to compile start pattern for context '{}'", context.name))?;

        let mut trackers = Vec::new();
        for tracker in &context.track {
            let regex = Regex::new(&tracker.pattern)
                .with_context(|| format!("Failed to compile tracker pattern '{}'", tracker.name))?;
            trackers.push(CompiledTracker {
                name: tracker.name.clone(),
                regex,
                capture_group: tracker.capture_group,
            });
        }

        let mut rules = Vec::new();
        for rule in &context.rules {
            let regex = Regex::new(&rule.pattern)
                .with_context(|| "Failed to compile rule pattern")?;

            let mut color_mappings = HashMap::new();
            for mapping in &rule.colors {
                color_mappings.insert(mapping.value.clone(), mapping.color.clone());
            }

            rules.push(CompiledContextRule {
                regex,
                state_key: rule.state_key.clone(),
                color_mappings,
                default_color: rule.default_color.clone(),
                priority: rule.priority,
            });
        }

        // Sort rules by priority (highest first)
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        let compiled = CompiledContext {
            name: context.name.clone(),
            start_regex,
            trackers,
            rules,
        };

        self.contexts.push(compiled);
        self.states.insert(context.name.clone(), ContextState::new());

        Ok(())
    }

    /// Process a line and update context states
    pub fn process_line(&mut self, line: &str) {
        // Check if any context starts
        for context in &self.contexts {
            if context.start_regex.is_match(line) {
                // Reset state for this context
                if let Some(state) = self.states.get_mut(&context.name) {
                    state.reset();
                }
            }
        }

        // Update state variables
        for context in &self.contexts {
            if let Some(state) = self.states.get_mut(&context.name) {
                for tracker in &context.trackers {
                    if let Some(cap) = tracker.regex.captures(line) {
                        if let Some(value) = cap.get(tracker.capture_group) {
                            state.set(tracker.name.clone(), value.as_str().to_string());
                        }
                    }
                }
            }
        }
    }

    /// Apply context-aware rules to a line
    pub fn apply_rules(
        &self,
        line: &str,
        palette_resolver: &impl Fn(&str) -> String,
    ) -> Vec<ColoredRange> {
        let mut colored_parts = Vec::new();

        for context in &self.contexts {
            if let Some(state) = self.states.get(&context.name) {
                for rule in &context.rules {
                    for cap in rule.regex.captures_iter(line) {
                        if let Some(m) = cap.get(0) {
                            // Determine color based on state
                            let color = if let Some(state_key) = &rule.state_key {
                                if let Some(state_value) = state.get(state_key) {
                                    // Look up color for this state value
                                    rule.color_mappings
                                        .get(state_value)
                                        .or(rule.default_color.as_ref())
                                        .map(|c| palette_resolver(c))
                                        .unwrap_or_else(|| DEFAULT_COLOR.to_string())
                                } else {
                                    // State variable not set yet, use default
                                    rule.default_color
                                        .as_ref()
                                        .map(|c| palette_resolver(c))
                                        .unwrap_or_else(|| DEFAULT_COLOR.to_string())
                                }
                            } else {
                                // No state dependency, use first color or default
                                rule.color_mappings
                                    .values()
                                    .next()
                                    .or(rule.default_color.as_ref())
                                    .map(|c| palette_resolver(c))
                                    .unwrap_or_else(|| DEFAULT_COLOR.to_string())
                            };

                            colored_parts.push(ColoredRange::new(m.start(), m.end(), color));
                        }
                    }
                }
            }
        }

        colored_parts
    }
}
