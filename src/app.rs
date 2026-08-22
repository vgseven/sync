//! Application orchestration for `check` and `update`.
//!
//! The flow is intentionally linear: discover manifests, parse declarations,
//! deduplicate registry lookups, evaluate report rows, then optionally write
//! safe updates. Parsing and writing remain separate so `check` cannot mutate
//! user files.

use crate::cli::{Cli, Command};
use crate::discovery::discover;
use crate::lockfile::planned_lockfile_note;
use crate::manifest::infer_kind;
use crate::model::{Dependency, Manifest, ReportRow, ReportStatus};
use crate::output::{Summary, print};
use crate::registry::{LookupKey, RegistryClient, normalize_lookup_name};
use crate::version::{replacement_for, requirement_status};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub async fn run() -> Result<u8> {
    let cli = Cli::parse_args();
    let command = cli.command.unwrap_or(Command::Check {
        packages: Vec::new(),
        fail_on_outdated: false,
    });

    let package_filter = PackageFilter::new(match &command {
        Command::Check { packages, .. } | Command::Update { packages, .. } => packages,
    });

    let manifest_paths = discover(&cli.path, cli.ecosystem, cli.recursive)?;
    if manifest_paths.is_empty() {
        bail!("no supported dependency manifests found");
    }

    let root = display_root(&cli.path)?;
    let manifests = load_manifests(&manifest_paths, &package_filter)?;
    let selected_count = manifests
        .iter()
        .map(|manifest| manifest.dependencies.len())
        .sum::<usize>();
    if selected_count == 0 {
        bail!("no matching dependencies found");
    }

    let client = RegistryClient::new(cli.pypi_url, cli.npm_registry, cli.timeout)?;
    // One request is made per normalized package and ecosystem, even when the
    // dependency appears in multiple manifests or dependency groups.
    let lookups = client
        .fetch_many(lookup_keys(&manifests), usize::from(cli.concurrency.max(1)))
        .await;

    let rows = build_rows(&manifests, &lookups, &root);

    match command {
        Command::Check {
            fail_on_outdated, ..
        } => {
            print(&rows, cli.format)?;
            let summary = Summary::from_rows(&rows);
            if summary.errors > 0 {
                Ok(2)
            } else if fail_on_outdated && summary.outdated > 0 {
                Ok(1)
            } else {
                Ok(0)
            }
        }
        Command::Update {
            dry_run, no_lock, ..
        } => {
            print(&rows, cli.format)?;
            let summary = Summary::from_rows(&rows);
            if summary.errors > 0 {
                bail!("registry errors must be resolved before updating manifests");
            }

            let updates = collect_updates(&rows);
            if updates.is_empty() {
                return Ok(0);
            }
            if dry_run {
                eprintln!(
                    "dry run: {} dependency declarations would be updated",
                    updates_len(&updates)
                );
                return Ok(0);
            }

            write_updates(&manifests, &updates)?;
            if !no_lock {
                print_lockfile_notes(&manifests, &updates);
            }
            eprintln!("updated {} dependency declarations", updates_len(&updates));
            Ok(0)
        }
    }
}

fn load_manifests(paths: &[PathBuf], filter: &PackageFilter) -> Result<Vec<Manifest>> {
    let mut manifests = Vec::new();
    for path in paths {
        let mut manifest = Manifest::load(path)?;
        manifest
            .dependencies
            .retain(|dependency| filter.matches(dependency));
        manifests.push(manifest);
    }
    Ok(manifests)
}

fn lookup_keys(manifests: &[Manifest]) -> Vec<LookupKey> {
    // BTreeSet both removes duplicate lookups and gives deterministic request
    // ordering, which keeps JSON/table reports stable between equivalent runs.
    let mut keys = BTreeSet::new();
    for manifest in manifests {
        for dependency in &manifest.dependencies {
            if dependency.skip_reason.is_some() {
                continue;
            }
            keys.insert((
                dependency.ecosystem,
                normalize_lookup_name(dependency.ecosystem, &dependency.lookup_name),
            ));
        }
    }
    keys.into_iter().collect()
}

fn build_rows(
    manifests: &[Manifest],
    lookups: &HashMap<LookupKey, Result<String, String>>,
    root: &Path,
) -> Vec<ReportRow> {
    let mut rows = Vec::new();

    for (manifest_index, manifest) in manifests.iter().enumerate() {
        for (dependency_index, dependency) in manifest.dependencies.iter().enumerate() {
            let manifest_path = display_path(root, &manifest.path);
            let key = (
                dependency.ecosystem,
                normalize_lookup_name(dependency.ecosystem, &dependency.lookup_name),
            );

            // Dependencies that cannot be safely rewritten are still reported,
            // but never trigger a registry request or an update attempt.
            let (latest, status, message, replacement) =
                if let Some(reason) = &dependency.skip_reason {
                    (None, ReportStatus::Skipped, Some(reason.clone()), None)
                } else {
                    match lookups.get(&key) {
                        Some(Ok(latest)) => {
                            let (status, message) = requirement_status(dependency, latest);
                            let replacement = replacement_for(dependency, latest).ok().flatten();
                            (Some(latest.clone()), status, message, replacement)
                        }
                        Some(Err(error)) => (None, ReportStatus::Error, Some(error.clone()), None),
                        None => (
                            None,
                            ReportStatus::Error,
                            Some("registry lookup was not executed".to_string()),
                            None,
                        ),
                    }
                };

            rows.push(ReportRow {
                ecosystem: dependency.ecosystem,
                manifest: manifest_path,
                group: dependency.group.clone(),
                package: dependency.name.clone(),
                current: dependency.requirement.clone(),
                latest,
                status,
                message,
                manifest_index,
                dependency_index,
                replacement,
            });
        }
    }

    rows
}

fn collect_updates(rows: &[ReportRow]) -> HashMap<usize, HashMap<usize, String>> {
    // Indices refer back to the parsed manifest/dependency vectors, avoiding
    // fragile source-text searches when rendering an update.
    let mut updates: HashMap<usize, HashMap<usize, String>> = HashMap::new();
    for row in rows {
        if row.status == ReportStatus::Outdated {
            if let Some(replacement) = &row.replacement {
                updates
                    .entry(row.manifest_index)
                    .or_default()
                    .insert(row.dependency_index, replacement.clone());
            }
        }
    }
    updates
}

fn write_updates(
    manifests: &[Manifest],
    updates: &HashMap<usize, HashMap<usize, String>>,
) -> Result<()> {
    for (manifest_index, dependency_updates) in updates {
        let manifest = manifests
            .get(*manifest_index)
            .context("invalid manifest update index")?;
        let rendered = manifest.render(dependency_updates)?;
        // Render first and write once per manifest. Registry errors are checked
        // before this function is called, preventing partial network results
        // from producing partial dependency updates.
        std::fs::write(&manifest.path, rendered)
            .with_context(|| format!("failed to write {}", manifest.path.display()))?;
    }
    Ok(())
}

fn print_lockfile_notes(manifests: &[Manifest], updates: &HashMap<usize, HashMap<usize, String>>) {
    let mut notes = HashSet::new();
    for manifest_index in updates.keys() {
        if let Some(manifest) = manifests.get(*manifest_index) {
            if let Some(note) = planned_lockfile_note(&manifest.path) {
                notes.insert(note);
            }
        }
    }

    // Several manifests may need the same follow-up, so only print each note once.
    for note in notes {
        eprintln!("note: {note}");
    }
}

fn updates_len(updates: &HashMap<usize, HashMap<usize, String>>) -> usize {
    updates.values().map(HashMap::len).sum()
}

fn display_root(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    if path.is_file() {
        Ok(path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")))
    } else {
        Ok(path)
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

struct PackageFilter {
    raw: HashSet<String>,
}

impl PackageFilter {
    fn new(packages: &[String]) -> Self {
        Self {
            raw: packages
                .iter()
                .map(|value| value.to_ascii_lowercase())
                .collect(),
        }
    }

    fn matches(&self, dependency: &Dependency) -> bool {
        if self.raw.is_empty() {
            return true;
        }

        // Compare both the declaration name and registry lookup name. This
        // allows users to select npm aliases as either local or target names.
        let normalized = normalize_lookup_name(dependency.ecosystem, &dependency.lookup_name)
            .to_ascii_lowercase();
        self.raw.contains(&dependency.name.to_ascii_lowercase()) || self.raw.contains(&normalized)
    }
}

#[allow(dead_code)]
fn _assert_supported_manifest(path: &Path) -> Result<()> {
    infer_kind(path).map(|_| ())
}
