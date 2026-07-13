use std::path::Path;

pub fn planned_lockfile_note(manifest_path: &Path) -> Option<String> {
    let file_name = manifest_path.file_name()?.to_str()?;
    match file_name {
        "package.json" => Some(
            "lockfile regeneration for npm, pnpm, yarn, and bun is planned but not implemented"
                .to_string(),
        ),
        "pyproject.toml" | "Pipfile" | "requirements.in" => Some(
            "lockfile regeneration for uv, Poetry, PDM, Pipenv, and pip-tools is planned but not implemented"
                .to_string(),
        ),
        _ => None,
    }
}
