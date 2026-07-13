use crate::cli::EcosystemFilter;
use crate::manifest::infer_kind;
use crate::model::{Ecosystem, ManifestKind};
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ROOT_MANIFESTS: &[&str] = &[
    "package.json",
    "pyproject.toml",
    "Pipfile",
    "setup.cfg",
    "requirements.txt",
    "requirements.in",
];

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "coverage",
    ".venv",
    "venv",
    "__pycache__",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
];

pub fn discover(path: &Path, ecosystem: EcosystemFilter, recursive: bool) -> Result<Vec<PathBuf>> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    if path.is_file() {
        let kind = infer_kind(&path)?;
        if ecosystem_matches(kind, ecosystem) {
            return Ok(vec![path]);
        }
        return Ok(Vec::new());
    }

    if !path.is_dir() {
        bail!("{} is not a file or directory", path.display());
    }

    let mut manifests = BTreeSet::new();
    collect_root_manifests(&path, ecosystem, &mut manifests)?;

    let requirements_dir = path.join("requirements");
    if requirements_dir.is_dir() {
        collect_requirements_files(&requirements_dir, ecosystem, &mut manifests)?;
    }

    if recursive {
        collect_recursive(&path, ecosystem, &mut manifests)?;
    }

    Ok(manifests.into_iter().collect())
}

fn collect_root_manifests(
    root: &Path,
    ecosystem: EcosystemFilter,
    manifests: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    for name in ROOT_MANIFESTS {
        let candidate = root.join(name);
        if candidate.is_file() {
            let kind = infer_kind(&candidate)?;
            if ecosystem_matches(kind, ecosystem) {
                manifests.insert(candidate);
            }
        }
    }
    Ok(())
}

fn collect_requirements_files(
    root: &Path,
    ecosystem: EcosystemFilter,
    manifests: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    if ecosystem == EcosystemFilter::Node {
        return Ok(());
    }

    for entry in
        std::fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && infer_kind(&path).ok() == Some(ManifestKind::Requirements) {
            manifests.insert(path);
        }
    }
    Ok(())
}

fn collect_recursive(
    root: &Path,
    ecosystem: EcosystemFilter,
    manifests: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    for entry in
        std::fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            collect_root_manifests(&path, ecosystem, manifests)?;
            collect_recursive(&path, ecosystem, manifests)?;
        } else if file_type.is_file() && infer_kind(&path).is_ok() {
            let kind = infer_kind(&path)?;
            if ecosystem_matches(kind, ecosystem) {
                manifests.insert(path);
            }
        }
    }
    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| SKIP_DIRS.iter().any(|skip| name.eq_ignore_ascii_case(skip)))
}

fn ecosystem_matches(kind: ManifestKind, filter: EcosystemFilter) -> bool {
    let ecosystem = match kind {
        ManifestKind::PackageJson => Ecosystem::Node,
        ManifestKind::PyProject
        | ManifestKind::Pipfile
        | ManifestKind::Requirements
        | ManifestKind::SetupCfg => Ecosystem::Python,
    };

    match filter {
        EcosystemFilter::All => true,
        EcosystemFilter::Python => ecosystem == Ecosystem::Python,
        EcosystemFilter::Node => ecosystem == Ecosystem::Node,
    }
}
