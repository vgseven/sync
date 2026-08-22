//! Shared data model passed between discovery, parsing, lookup, and output.
//!
//! This module has no filesystem or network behavior. Keeping the model pure
//! makes format-specific parsers and report generation easier to reason about.

use serde::Serialize;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Python,
    Node,
}

impl fmt::Display for Ecosystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Python => formatter.write_str("python"),
            Self::Node => formatter.write_str("node"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequirementSyntax {
    Node,
    Pep508,
    PythonConstraint,
}

#[derive(Clone, Debug)]
pub enum DependencyLocation {
    /// JSON object member, used when re-rendering `package.json`.
    Json { section: String, key: String },
    /// TOML array item, used for PEP 621-style dependency arrays.
    TomlArray { path: Vec<String>, index: usize },
    /// TOML table key, optionally targeting a nested `version` field.
    TomlKey {
        path: Vec<String>,
        key: String,
        field: Option<String>,
    },
    /// Byte range within a text manifest such as requirements.txt or setup.cfg.
    TextSpan { start: usize, end: usize },
}

#[derive(Clone, Debug)]
pub struct Dependency {
    /// Name as written in the manifest and shown to the user.
    pub name: String,
    /// Registry name after handling aliases and ecosystem-specific normalization.
    pub lookup_name: String,
    pub ecosystem: Ecosystem,
    pub group: String,
    pub requirement: String,
    pub raw: String,
    pub syntax: RequirementSyntax,
    pub location: DependencyLocation,
    pub skip_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestKind {
    PackageJson,
    PyProject,
    Pipfile,
    Requirements,
    SetupCfg,
}

#[derive(Debug)]
pub struct Manifest {
    pub path: PathBuf,
    pub kind: ManifestKind,
    pub original: String,
    pub dependencies: Vec<Dependency>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Current,
    Outdated,
    Ahead,
    Unconstrained,
    Skipped,
    Error,
}

impl fmt::Display for ReportStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Current => "current",
            Self::Outdated => "outdated",
            Self::Ahead => "ahead",
            Self::Unconstrained => "unconstrained",
            Self::Skipped => "skipped",
            Self::Error => "error",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportRow {
    pub ecosystem: Ecosystem,
    pub manifest: String,
    pub group: String,
    pub package: String,
    pub current: String,
    pub latest: Option<String>,
    pub status: ReportStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    // These fields connect a report row back to a parsed source declaration.
    // They stay out of JSON output because they are implementation details.
    #[serde(skip)]
    pub manifest_index: usize,
    #[serde(skip)]
    pub dependency_index: usize,
    #[serde(skip)]
    pub replacement: Option<String>,
}
