//! Manifest parsing and safe source rendering.
//!
//! Parsers produce a common `Dependency` model plus a precise source location.
//! Renderers use that location to update only approved declarations while
//! preserving unrelated TOML/text content where the format permits it.

use crate::model::{
    Dependency, DependencyLocation, Ecosystem, Manifest, ManifestKind, RequirementSyntax,
};
use crate::version::{node_registry_target, parse_pep508};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Value as TomlValue};

impl Manifest {
    /// Read one supported manifest and retain its original text for update mode.
    pub fn load(path: &Path) -> Result<Self> {
        let kind = infer_kind(path)?;
        let original = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let dependencies = match kind {
            ManifestKind::PackageJson => parse_package_json(&original, path)?,
            ManifestKind::PyProject => parse_pyproject(&original, path)?,
            ManifestKind::Pipfile => parse_pipfile(&original, path)?,
            ManifestKind::Requirements => parse_requirements(&original),
            ManifestKind::SetupCfg => parse_setup_cfg(&original),
        };

        Ok(Self {
            path: path.to_path_buf(),
            kind,
            original,
            dependencies,
        })
    }

    /// Render requested dependency replacements using the source format's
    /// native structure rather than doing a global text replacement.
    pub fn render(&self, updates: &HashMap<usize, String>) -> Result<String> {
        match self.kind {
            ManifestKind::PackageJson => {
                render_package_json(&self.original, &self.dependencies, updates)
            }
            ManifestKind::PyProject | ManifestKind::Pipfile => {
                render_toml(&self.original, &self.dependencies, updates)
            }
            ManifestKind::Requirements | ManifestKind::SetupCfg => {
                render_text(&self.original, &self.dependencies, updates)
            }
        }
    }
}

pub fn infer_kind(path: &Path) -> Result<ManifestKind> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .with_context(|| format!("invalid manifest path {}", path.display()))?;
    let lower = name.to_ascii_lowercase();

    match lower.as_str() {
        "package.json" => Ok(ManifestKind::PackageJson),
        "pyproject.toml" => Ok(ManifestKind::PyProject),
        "pipfile" => Ok(ManifestKind::Pipfile),
        "setup.cfg" => Ok(ManifestKind::SetupCfg),
        _ if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("txt" | "in")
        ) =>
        {
            Ok(ManifestKind::Requirements)
        }
        _ => bail!("{} is not a supported dependency manifest", path.display()),
    }
}

fn parse_package_json(content: &str, path: &Path) -> Result<Vec<Dependency>> {
    let document: JsonValue = serde_json::from_str(content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let mut dependencies = Vec::new();

    // Keep dependency groups distinct: the same package can intentionally use
    // a different constraint in production and development sections.
    for section in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        let Some(values) = document.get(section).and_then(JsonValue::as_object) else {
            continue;
        };
        for (name, value) in values {
            let Some(requirement) = value.as_str() else {
                continue;
            };
            let (lookup_name, skip_reason) = node_registry_target(name, requirement);
            dependencies.push(Dependency {
                name: name.clone(),
                lookup_name,
                ecosystem: Ecosystem::Node,
                group: section.to_string(),
                requirement: requirement.to_string(),
                raw: requirement.to_string(),
                syntax: RequirementSyntax::Node,
                location: DependencyLocation::Json {
                    section: section.to_string(),
                    key: name.clone(),
                },
                skip_reason,
            });
        }
    }

    Ok(dependencies)
}

fn parse_pyproject(content: &str, path: &Path) -> Result<Vec<Dependency>> {
    let document = content
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let mut dependencies = Vec::new();

    // PEP 621, PEP 735, and Poetry store requirements in different shapes.
    // Normalize all supported shapes into the shared dependency model.
    collect_pep508_array(
        &document,
        &["project", "dependencies"],
        "project.dependencies",
        &mut dependencies,
    )?;

    if let Some(optional) =
        item_at_path(&document, &["project", "optional-dependencies"]).and_then(Item::as_table_like)
    {
        for (group, _) in optional.iter() {
            let path = vec!["project", "optional-dependencies", group];
            collect_pep508_array(
                &document,
                &path,
                &format!("project.optional-dependencies.{group}"),
                &mut dependencies,
            )?;
        }
    }

    if let Some(groups) =
        item_at_path(&document, &["dependency-groups"]).and_then(Item::as_table_like)
    {
        for (group, _) in groups.iter() {
            let path = vec!["dependency-groups", group];
            collect_pep508_array(
                &document,
                &path,
                &format!("dependency-groups.{group}"),
                &mut dependencies,
            )?;
        }
    }

    collect_python_map(
        &document,
        &["tool", "poetry", "dependencies"],
        "tool.poetry.dependencies",
        &mut dependencies,
    );
    collect_python_map(
        &document,
        &["tool", "poetry", "dev-dependencies"],
        "tool.poetry.dev-dependencies",
        &mut dependencies,
    );

    if let Some(groups) =
        item_at_path(&document, &["tool", "poetry", "group"]).and_then(Item::as_table_like)
    {
        for (group, _) in groups.iter() {
            let path = vec!["tool", "poetry", "group", group, "dependencies"];
            collect_python_map(
                &document,
                &path,
                &format!("tool.poetry.group.{group}.dependencies"),
                &mut dependencies,
            );
        }
    }

    Ok(dependencies)
}

fn parse_pipfile(content: &str, path: &Path) -> Result<Vec<Dependency>> {
    let document = content
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let mut dependencies = Vec::new();
    collect_python_map(&document, &["packages"], "packages", &mut dependencies);
    collect_python_map(
        &document,
        &["dev-packages"],
        "dev-packages",
        &mut dependencies,
    );
    Ok(dependencies)
}

fn collect_pep508_array(
    document: &DocumentMut,
    path: &[&str],
    group: &str,
    dependencies: &mut Vec<Dependency>,
) -> Result<()> {
    let Some(array) = item_at_path(document, path).and_then(Item::as_array) else {
        return Ok(());
    };

    for (index, value) in array.iter().enumerate() {
        let Some(raw) = value.as_str() else {
            continue;
        };
        let parsed = parse_pep508(raw)
            .with_context(|| format!("invalid PEP 508 dependency '{raw}' in {}", path.join(".")))?;
        dependencies.push(Dependency {
            name: parsed.name.clone(),
            lookup_name: parsed.name,
            ecosystem: Ecosystem::Python,
            group: group.to_string(),
            requirement: parsed.specifier.clone(),
            raw: raw.to_string(),
            syntax: RequirementSyntax::Pep508,
            location: DependencyLocation::TomlArray {
                path: path.iter().map(|value| (*value).to_string()).collect(),
                index,
            },
            skip_reason: parsed
                .direct
                .then(|| "direct URL dependencies are not rewritten".to_string()),
        });
    }
    Ok(())
}

fn collect_python_map(
    document: &DocumentMut,
    path: &[&str],
    group: &str,
    dependencies: &mut Vec<Dependency>,
) {
    let Some(table) = item_at_path(document, path).and_then(Item::as_table_like) else {
        return;
    };

    for (name, item) in table.iter() {
        if name.eq_ignore_ascii_case("python") {
            continue;
        }

        // Poetry-style tables may declare version plus path/git/url metadata.
        // Preserve the version for reporting but mark external sources unsafe
        // for automatic replacement.
        let (requirement, field, skip_reason) = if let Some(value) = item.as_str() {
            (value.to_string(), None, None)
        } else if let Some(inline) = item.as_inline_table() {
            let source = ["path", "git", "url", "file"]
                .iter()
                .find(|key| inline.contains_key(**key));
            let version = inline
                .get("version")
                .and_then(TomlValue::as_str)
                .unwrap_or("*")
                .to_string();
            (
                version,
                Some("version".to_string()),
                source.map(|_| "path, URL, and git dependencies are not rewritten".to_string()),
            )
        } else if let Some(table) = item.as_table_like() {
            let source = ["path", "git", "url", "file"]
                .iter()
                .find(|key| table.contains_key(**key));
            let version = table
                .get("version")
                .and_then(Item::as_str)
                .unwrap_or("*")
                .to_string();
            (
                version,
                Some("version".to_string()),
                source.map(|_| "path, URL, and git dependencies are not rewritten".to_string()),
            )
        } else {
            continue;
        };

        dependencies.push(Dependency {
            name: name.to_string(),
            lookup_name: name.to_string(),
            ecosystem: Ecosystem::Python,
            group: group.to_string(),
            requirement: requirement.clone(),
            raw: requirement,
            syntax: RequirementSyntax::PythonConstraint,
            location: DependencyLocation::TomlKey {
                path: path.iter().map(|value| (*value).to_string()).collect(),
                key: name.to_string(),
                field,
            },
            skip_reason,
        });
    }
}

fn parse_requirements(content: &str) -> Vec<Dependency> {
    let mut dependencies = Vec::new();
    let mut offset = 0;

    for line in content.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        let line_without_newline = line_without_newline
            .strip_suffix('\r')
            .unwrap_or(line_without_newline);
        // Offsets are retained so rendering can replace this exact declaration
        // without disturbing comments or adjacent lines.
        collect_text_dependency(
            line_without_newline,
            offset,
            "requirements",
            &mut dependencies,
        );
        offset += line.len();
    }

    if content.is_empty() || content.ends_with('\n') {
        return dependencies;
    }
    dependencies
}

fn parse_setup_cfg(content: &str) -> Vec<Dependency> {
    let mut dependencies = Vec::new();
    let mut section = String::new();
    let mut active_group: Option<String> = None;
    let mut offset = 0;

    for line in content.split_inclusive('\n') {
        let value = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or_else(|| line.strip_suffix('\n').unwrap_or(line));
        let trimmed = value.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].to_ascii_lowercase();
            active_group = None;
            offset += line.len();
            continue;
        }

        let indented = value.starts_with(char::is_whitespace);
        if !indented {
            active_group = setup_cfg_assignment(value, &section).map(|(group, inline)| {
                if let Some((start, expression)) = inline {
                    collect_text_dependency(expression, offset + start, &group, &mut dependencies);
                }
                group
            });
        } else if let Some(group) = &active_group {
            collect_text_dependency(value, offset, group, &mut dependencies);
        }

        offset += line.len();
    }
    dependencies
}

fn setup_cfg_assignment<'a>(
    line: &'a str,
    section: &str,
) -> Option<(String, Option<(usize, &'a str)>)> {
    let separator = line.find('=').or_else(|| line.find(':'))?;
    let key = line[..separator].trim();
    let enabled = match section {
        "options" => matches!(key, "install_requires" | "setup_requires" | "tests_require"),
        "options.extras_require" => true,
        _ => false,
    };
    if !enabled {
        return None;
    }

    let group = if section == "options.extras_require" {
        format!("options.extras_require.{key}")
    } else {
        format!("options.{key}")
    };
    let tail = &line[separator + 1..];
    let leading = tail.len() - tail.trim_start().len();
    let expression = tail.trim();
    let inline = (!expression.is_empty()).then_some((separator + 1 + leading, expression));
    Some((group, inline))
}

fn collect_text_dependency(
    line: &str,
    line_offset: usize,
    group: &str,
    dependencies: &mut Vec<Dependency>,
) {
    let leading = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with('-')
        || trimmed.starts_with("--")
    {
        return;
    }

    // A comment only starts after whitespace so URL fragments and quoted marker
    // values are not accidentally truncated.
    let comment = trimmed
        .find(" #")
        .or_else(|| trimmed.find("\t#"))
        .unwrap_or(trimmed.len());
    let mut expression = trimmed[..comment].trim_end();
    if let Some(without_continuation) = expression.strip_suffix('\\') {
        expression = without_continuation.trim_end();
    }
    expression = expression.trim_end_matches(',').trim_end();
    if expression.is_empty() {
        return;
    }

    let Some(parsed) = parse_pep508(expression) else {
        return;
    };
    let start = line_offset + leading;
    let end = start + expression.len();
    dependencies.push(Dependency {
        name: parsed.name.clone(),
        lookup_name: parsed.name,
        ecosystem: Ecosystem::Python,
        group: group.to_string(),
        requirement: parsed.specifier.clone(),
        raw: expression.to_string(),
        syntax: RequirementSyntax::Pep508,
        location: DependencyLocation::TextSpan { start, end },
        skip_reason: parsed
            .direct
            .then(|| "direct URL dependencies are not rewritten".to_string()),
    });
}

fn render_package_json(
    content: &str,
    dependencies: &[Dependency],
    updates: &HashMap<usize, String>,
) -> Result<String> {
    let mut document: JsonValue = serde_json::from_str(content)?;
    for (index, replacement) in updates {
        let dependency = dependencies
            .get(*index)
            .context("invalid package.json dependency update")?;
        let DependencyLocation::Json { section, key } = &dependency.location else {
            bail!("invalid package.json dependency location");
        };
        let value = document
            .get_mut(section)
            .and_then(JsonValue::as_object_mut)
            .and_then(|table| table.get_mut(key))
            .with_context(|| format!("could not find {section}.{key}"))?;
        *value = JsonValue::String(replacement.clone());
    }

    // JSON has no comments, but preserving indentation and line endings avoids
    // unnecessary formatting churn in an otherwise small dependency update.
    let indent = detect_json_indent(content);
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let writer = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(writer, formatter);
    document.serialize(&mut serializer)?;
    let mut rendered = String::from_utf8(serializer.into_inner())?;
    if content.contains("\r\n") {
        rendered = rendered.replace('\n', "\r\n");
    }
    if content.ends_with('\n') {
        rendered.push_str(if content.ends_with("\r\n") {
            "\r\n"
        } else {
            "\n"
        });
    }
    Ok(rendered)
}

fn detect_json_indent(content: &str) -> String {
    for line in content.lines().skip(1) {
        let whitespace = line
            .chars()
            .take_while(|character| character.is_whitespace())
            .collect::<String>();
        if !whitespace.is_empty() && !line.trim().is_empty() {
            return whitespace;
        }
    }
    "  ".to_string()
}

fn render_toml(
    content: &str,
    dependencies: &[Dependency],
    updates: &HashMap<usize, String>,
) -> Result<String> {
    let mut document = content.parse::<DocumentMut>()?;
    for (index, replacement) in updates {
        let dependency = dependencies
            .get(*index)
            .context("invalid TOML dependency update")?;
        match &dependency.location {
            DependencyLocation::TomlArray { path, index } => {
                let item = item_at_path_mut_owned(&mut document, path)
                    .with_context(|| format!("could not find {}", path.join(".")))?;
                let value = item
                    .as_array_mut()
                    .and_then(|array| array.get_mut(*index))
                    .context("could not find TOML dependency array item")?;
                replace_toml_value(value, replacement);
            }
            DependencyLocation::TomlKey { path, key, field } => {
                let table = item_at_path_mut_owned(&mut document, path)
                    .and_then(Item::as_table_like_mut)
                    .with_context(|| format!("could not find {}", path.join(".")))?;
                let item = table
                    .get_mut(key)
                    .with_context(|| format!("could not find {}.{key}", path.join(".")))?;
                if let Some(field) = field {
                    if let Some(inline) = item.as_inline_table_mut() {
                        let value = inline
                            .get_mut(field)
                            .with_context(|| format!("could not find version for {key}"))?;
                        replace_toml_value(value, replacement);
                    } else {
                        let nested = item
                            .as_table_like_mut()
                            .and_then(|table| table.get_mut(field))
                            .with_context(|| format!("could not find version for {key}"))?;
                        let value = nested
                            .as_value_mut()
                            .with_context(|| format!("version for {key} is not a TOML value"))?;
                        replace_toml_value(value, replacement);
                    }
                } else {
                    let value = item
                        .as_value_mut()
                        .with_context(|| format!("{key} is not a TOML value"))?;
                    replace_toml_value(value, replacement);
                }
            }
            _ => bail!("invalid TOML dependency location"),
        }
    }
    Ok(document.to_string())
}

fn replace_toml_value(value: &mut TomlValue, replacement: &str) {
    let decor = value.decor().clone();
    *value = TomlValue::from(replacement);
    *value.decor_mut() = decor;
}

fn render_text(
    content: &str,
    dependencies: &[Dependency],
    updates: &HashMap<usize, String>,
) -> Result<String> {
    let mut edits = Vec::new();
    for (index, replacement) in updates {
        let dependency = dependencies
            .get(*index)
            .context("invalid text dependency update")?;
        let DependencyLocation::TextSpan { start, end } = dependency.location else {
            bail!("invalid text dependency location");
        };
        edits.push((start, end, replacement));
    }
    // Apply from the end of the file so replacing one span never invalidates
    // byte offsets recorded for an earlier declaration.
    edits.sort_by(|left, right| right.0.cmp(&left.0));

    let mut rendered = content.to_string();
    for (start, end, replacement) in edits {
        rendered.replace_range(start..end, replacement);
    }
    Ok(rendered)
}

fn item_at_path<'a>(document: &'a DocumentMut, path: &[&str]) -> Option<&'a Item> {
    let (first, rest) = path.split_first()?;
    let mut item = document.get(first)?;
    for key in rest {
        item = item.get(key)?;
    }
    Some(item)
}

fn item_at_path_mut_owned<'a>(
    document: &'a mut DocumentMut,
    path: &[String],
) -> Option<&'a mut Item> {
    let (first, rest) = path.split_first()?;
    let mut item = document.get_mut(first)?;
    for key in rest {
        item = item.get_mut(key)?;
    }
    Some(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_node_dependency_sections() {
        let manifest = r#"{
  "dependencies": { "react": "^18.2.0" },
  "devDependencies": { "typescript": "~5.5.0" },
  "optionalDependencies": { "fsevents": "^2.3.3" },
  "peerDependencies": { "react-dom": ">=18.0.0" }
}
"#;
        let dependencies = parse_package_json(manifest, Path::new("package.json")).unwrap();
        assert_eq!(dependencies.len(), 4);
    }

    #[test]
    fn parses_pep621_poetry_and_dependency_groups() {
        let manifest = r#"
[project]
dependencies = ["requests>=2.31"]

[project.optional-dependencies]
test = ["pytest==8.0"]

[dependency-groups]
lint = ["ruff>=0.5"]

[tool.poetry.dependencies]
httpx = "^0.27"

[tool.poetry.group.dev.dependencies]
mypy = { version = "~1.10", extras = ["dmypy"] }
"#;
        let dependencies = parse_pyproject(manifest, Path::new("pyproject.toml")).unwrap();
        assert_eq!(dependencies.len(), 5);
    }

    #[test]
    fn text_render_preserves_comments_and_markers() {
        let content = "requests>=2.31 ; python_version > '3.10'  # API\n";
        let dependencies = parse_requirements(content);
        let mut updates = HashMap::new();
        updates.insert(0, "requests>=2.32 ; python_version > '3.10'".to_string());
        let rendered = render_text(content, &dependencies, &updates).unwrap();
        assert_eq!(
            rendered,
            "requests>=2.32 ; python_version > '3.10'  # API\n"
        );
    }

    #[test]
    fn setup_cfg_reads_install_and_extra_requirements() {
        let content = r#"
[options]
install_requires =
    requests>=2.31

[options.extras_require]
test =
    pytest==8.0
"#;
        let dependencies = parse_setup_cfg(content);
        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies[1].group, "options.extras_require.test");
    }
}
