//! Command-line argument definitions.
//!
//! Clap owns user-facing help text and validation, while `app` owns command
//! behavior. Keeping the two separate makes CLI changes easier to test.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "relay-sync",
    version,
    about = "Audit and update Python and Node.js dependency manifests",
    long_about = None,
    arg_required_else_help = false
)]
pub struct Cli {
    /// Project directory or a specific supported manifest.
    #[arg(short, long, global = true, default_value = ".")]
    pub path: PathBuf,

    /// Limit discovery to one ecosystem.
    #[arg(long, global = true, value_enum, default_value_t = EcosystemFilter::All)]
    pub ecosystem: EcosystemFilter,

    /// Scan nested directories for supported dependency manifests.
    #[arg(short, long, global = true)]
    pub recursive: bool,

    /// Output format for dependency reports.
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,

    /// Maximum number of concurrent registry requests.
    #[arg(long, global = true, default_value_t = 16, value_parser = clap::value_parser!(u16).range(1..=128))]
    pub concurrency: u16,

    /// Per-request timeout in seconds.
    #[arg(long, global = true, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..=300))]
    pub timeout: u64,

    /// PyPI-compatible JSON API base URL.
    #[arg(long, global = true, default_value = "https://pypi.org/pypi")]
    pub pypi_url: String,

    /// npm-compatible registry base URL.
    #[arg(long, global = true, default_value = "https://registry.npmjs.org")]
    pub npm_registry: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum EcosystemFilter {
    #[default]
    All,
    Python,
    Node,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Report the latest registry versions without changing files.
    Check {
        /// Only check these package names.
        packages: Vec<String>,

        /// Exit with status 1 when an update is available.
        #[arg(long)]
        fail_on_outdated: bool,
    },

    /// Update supported dependency declarations to the latest versions.
    Update {
        /// Only update these package names.
        packages: Vec<String>,

        /// Show planned changes without writing files or running lock commands.
        #[arg(long)]
        dry_run: bool,

        /// Do not regenerate existing lockfiles.
        #[arg(long)]
        no_lock: bool,
    },
}

impl Cli {
    /// Parse process arguments through Clap so all commands share validation.
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
