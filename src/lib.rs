#![warn(clippy::all, clippy::pedantic)]
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_literal_bound,
    clippy::module_name_repetitions,
    clippy::struct_field_names,
    clippy::must_use_candidate,
    clippy::new_without_default,
    clippy::return_self_not_must_use,
    dead_code
)]

use clap::Subcommand;
use serde::{Deserialize, Serialize};

// ── Always compiled (tiny tier) ────────────────────────────────
pub mod agent;
pub mod channels;
pub mod config;
pub mod health;
pub mod identity;
pub mod memory;
pub mod observability;
pub mod onboard;
pub mod providers;
pub mod runtime;
pub mod security;
pub mod session;
pub mod skills;
pub mod tools;
pub mod util;

// ── Standard tier (+TUI) ──────────────────────────────────────
#[cfg(feature = "tui-feature")]
pub mod tui;

// ── Full tier ──────────────────────────────────────────────────
#[cfg(feature = "gateway-feature")]
pub mod gateway;
#[cfg(feature = "daemon-feature")]
pub mod daemon;
#[cfg(feature = "daemon-feature")]
pub mod cron;
#[cfg(feature = "daemon-feature")]
pub mod doctor;
#[cfg(feature = "daemon-feature")]
pub mod heartbeat;
#[cfg(feature = "daemon-feature")]
pub mod service;
#[cfg(feature = "full")]
pub mod integrations;
#[cfg(feature = "full")]
pub mod migration;
#[cfg(feature = "skillforge-feature")]
pub mod skillforge;
#[cfg(feature = "tunnel-feature")]
pub mod tunnel;

pub use config::Config;

/// Service management subcommands
#[cfg(feature = "daemon-feature")]
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServiceCommands {
    /// Install daemon service unit for auto-start and restart
    Install,
    /// Start daemon service
    Start,
    /// Stop daemon service
    Stop,
    /// Check daemon service status
    Status,
    /// Uninstall daemon service unit
    Uninstall,
}

/// Channel management subcommands
#[cfg(feature = "channels-feature")]
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChannelCommands {
    /// List all configured channels
    List,
    /// Start all configured channels (handled in main.rs for async)
    Start,
    /// Run health checks for configured channels (handled in main.rs for async)
    Doctor,
    /// Add a new channel configuration
    Add {
        /// Channel type (telegram, discord, slack, whatsapp, matrix, imessage, email)
        channel_type: String,
        /// Optional configuration as JSON
        config: String,
    },
    /// Remove a channel configuration
    Remove {
        /// Channel name to remove
        name: String,
    },
}

/// Skills management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillCommands {
    /// List all installed skills
    List,
    /// Install a new skill from a URL or local path
    Install {
        /// Source URL or local path
        source: String,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name to remove
        name: String,
    },
}

/// Migration subcommands
#[cfg(feature = "full")]
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MigrateCommands {
    /// Import memory from an `OpenClaw` workspace into this `TinyClaw` workspace
    Openclaw {
        /// Optional path to `OpenClaw` workspace (defaults to ~/.openclaw/workspace)
        #[arg(long)]
        source: Option<std::path::PathBuf>,

        /// Validate and preview migration without writing any data
        #[arg(long)]
        dry_run: bool,
    },
}

/// Cron subcommands
#[cfg(feature = "daemon-feature")]
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CronCommands {
    /// List all scheduled tasks
    List,
    /// Add a new scheduled task
    Add {
        /// Cron expression
        expression: String,
        /// Command to run
        command: String,
    },
    /// Remove a scheduled task
    Remove {
        /// Task ID
        id: String,
    },
}

/// Integration subcommands
#[cfg(feature = "full")]
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntegrationCommands {
    /// Show details about a specific integration
    Info {
        /// Integration name
        name: String,
    },
}
