//! Dependency declaration parsing, comparison, and conservative replacement.
//!
//! Version ranges are intentionally handled as a small safe subset. When a
//! declaration cannot be proven safe to rewrite, this module reports it as
//! skipped instead of guessing at package-manager semantics.

use crate::model::{Dependency, ReportStatus, RequirementSyntax};
use std::cmp::Ordering;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pep508Parts {
    pub name: String,
    pub specifier: String,
    pub spec_start: usize,
    pub spec_end: usize,
    pub direct: bool,
}

pub fn parse_pep508(value: &str) -> Option<Pep508Parts> {
    let leading = value.len() - value.trim_start().len();
    let body = &value[leading..];
    let name_len = body
        .char_indices()
        .take_while(|(_, character)| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .map(|(index, character)| index + character.len_utf8())
        .last()?;

    let name = body[..name_len].to_string();
    let mut cursor = leading + name_len;

    if value[cursor..].starts_with('[') {
        let extras_end = value[cursor..].find(']')?;
        cursor += extras_end + 1;
    }

    while value[cursor..].starts_with(char::is_whitespace) {
        cursor += value[cursor..].chars().next()?.len_utf8();
    }

    // Environment markers begin at an unquoted semicolon and must survive a
    // version replacement unchanged.
    let marker = find_unquoted_marker(&value[cursor..], ';')
        .map(|offset| cursor + offset)
        .unwrap_or(value.len());
    let requirement_body = &value[cursor..marker];
    let requirement_start = requirement_body.len() - requirement_body.trim_start().len();
    let requirement_end = requirement_body.trim_end().len();
    let spec_start = cursor + requirement_start;
    let spec_end = cursor + requirement_end;
    let specifier = value[spec_start..spec_end].to_string();

    Some(Pep508Parts {
        name,
        direct: specifier.starts_with('@'),
        specifier,
        spec_start,
        spec_end,
    })
}

fn find_unquoted_marker(value: &str, marker: char) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;

    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if character == marker && quote.is_none() {
            return Some(index);
        }
    }
    None
}

pub fn node_registry_target(name: &str, requirement: &str) -> (String, Option<String>) {
    let trimmed = requirement.trim();
    let lower = trimmed.to_ascii_lowercase();
    let local_prefixes = [
        "workspace:",
        "file:",
        "link:",
        "portal:",
        "patch:",
        "git:",
        "git+",
        "github:",
        "gitlab:",
        "bitbucket:",
        "http:",
        "https:",
    ];

    // These values point outside the public npm registry, so neither lookup nor
    // automatic rewrite is meaningful.
    if local_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        return (
            name.to_string(),
            Some("local, workspace, URL, and git dependencies are not rewritten".to_string()),
        );
    }

    if let Some(alias) = trimmed.strip_prefix("npm:") {
        let (target, _) = split_npm_alias(alias);
        return (target.to_string(), None);
    }

    (name.to_string(), None)
}

fn split_npm_alias(alias: &str) -> (&str, Option<&str>) {
    if let Some(index) = alias.rfind('@').filter(|index| *index > 0) {
        (&alias[..index], Some(&alias[index + 1..]))
    } else {
        (alias, None)
    }
}

pub fn requirement_status(dependency: &Dependency, latest: &str) -> (ReportStatus, Option<String>) {
    if let Some(reason) = &dependency.skip_reason {
        return (ReportStatus::Skipped, Some(reason.clone()));
    }

    if is_unconstrained(dependency.syntax, &dependency.requirement) {
        return (ReportStatus::Unconstrained, None);
    }

    let Some(anchor) = requirement_anchor(dependency.syntax, &dependency.raw) else {
        return (
            ReportStatus::Skipped,
            Some("the version range is too complex to rewrite safely".to_string()),
        );
    };

    match compare_versions(&anchor, latest) {
        Ordering::Less => (ReportStatus::Outdated, None),
        Ordering::Equal => (ReportStatus::Current, None),
        Ordering::Greater => (
            ReportStatus::Ahead,
            Some("the declared version is newer than the registry latest tag".to_string()),
        ),
    }
}

pub fn replacement_for(dependency: &Dependency, latest: &str) -> Result<Option<String>, String> {
    let (status, message) = requirement_status(dependency, latest);
    if status != ReportStatus::Outdated {
        return if status == ReportStatus::Skipped {
            Err(message.unwrap_or_else(|| "dependency cannot be updated safely".to_string()))
        } else {
            Ok(None)
        };
    }

    let replacement = match dependency.syntax {
        RequirementSyntax::Node => update_node_requirement(&dependency.raw, latest)?,
        RequirementSyntax::PythonConstraint => {
            update_single_constraint(&dependency.raw, latest, ConstraintKind::Python)?
        }
        RequirementSyntax::Pep508 => {
            let parts = parse_pep508(&dependency.raw)
                .ok_or_else(|| "invalid PEP 508 dependency".to_string())?;
            if parts.direct {
                return Err("direct URL dependencies are not rewritten".to_string());
            }
            let constraint =
                update_single_constraint(&parts.specifier, latest, ConstraintKind::Python)?;
            format!(
                "{}{}{}",
                &dependency.raw[..parts.spec_start],
                constraint,
                &dependency.raw[parts.spec_end..]
            )
        }
    };

    Ok(Some(replacement))
}

fn update_node_requirement(requirement: &str, latest: &str) -> Result<String, String> {
    let leading_len = requirement.len() - requirement.trim_start().len();
    let trailing_start = requirement.trim_end().len();
    let leading = &requirement[..leading_len];
    let trailing = &requirement[trailing_start..];
    let trimmed = requirement.trim();

    if let Some(alias) = trimmed.strip_prefix("npm:") {
        let (target, specifier) = split_npm_alias(alias);
        let specifier = specifier.ok_or_else(|| "npm alias has no version range".to_string())?;
        let updated = update_single_constraint(specifier, latest, ConstraintKind::Node)?;
        return Ok(format!("{leading}npm:{target}@{updated}{trailing}"));
    }

    let updated = update_single_constraint(trimmed, latest, ConstraintKind::Node)?;
    Ok(format!("{leading}{updated}{trailing}"))
}

#[derive(Clone, Copy)]
enum ConstraintKind {
    Node,
    Python,
}

fn update_single_constraint(
    constraint: &str,
    latest: &str,
    kind: ConstraintKind,
) -> Result<String, String> {
    let trimmed = constraint.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return Err("unconstrained dependencies are left unchanged".to_string());
    }
    // A compound range can encode an intentional compatibility policy. Never
    // collapse it to the registry latest version automatically.
    if has_compound_range(trimmed) {
        return Err("compound version ranges are not rewritten".to_string());
    }

    let allowed = match kind {
        ConstraintKind::Node => ["^", "~", ">=", "=", "", ""],
        ConstraintKind::Python => ["===", "==", "~=", ">=", "^", "~"],
    };

    let mut operator = "";
    for candidate in ["===", "==", "~=", ">=", "<=", "!=", "^", "~", ">", "<", "="] {
        if trimmed.starts_with(candidate) {
            operator = candidate;
            break;
        }
    }

    let operator_allowed = operator.is_empty() || allowed.contains(&operator);
    if !operator_allowed {
        return Err(format!(
            "the '{operator}' constraint is not safe to rewrite"
        ));
    }

    let remainder = trimmed[operator.len()..].trim_start();
    if remainder.is_empty()
        || remainder.contains('*')
        || remainder.contains('x')
        || remainder.contains('X')
        || remainder.chars().any(char::is_whitespace)
    {
        return Err("wildcard or non-standard version constraints are not rewritten".to_string());
    }

    let spacing = &trimmed[operator.len()..trimmed.len() - remainder.len()];
    Ok(format!("{operator}{spacing}{latest}"))
}

fn has_compound_range(value: &str) -> bool {
    value.contains(',')
        || value.contains("||")
        || value.contains(" - ")
        || value
            .split_whitespace()
            .skip(1)
            .any(|part| starts_with_operator(part))
}

fn starts_with_operator(value: &str) -> bool {
    ["===", "==", "~=", ">=", "<=", "!=", "^", "~", ">", "<", "="]
        .iter()
        .any(|operator| value.starts_with(operator))
}

fn requirement_anchor(syntax: RequirementSyntax, raw: &str) -> Option<String> {
    match syntax {
        RequirementSyntax::Pep508 => {
            let parts = parse_pep508(raw)?;
            if parts.direct {
                None
            } else {
                constraint_anchor(&parts.specifier)
            }
        }
        RequirementSyntax::Node => {
            let trimmed = raw.trim();
            if let Some(alias) = trimmed.strip_prefix("npm:") {
                split_npm_alias(alias).1.and_then(constraint_anchor)
            } else {
                constraint_anchor(trimmed)
            }
        }
        RequirementSyntax::PythonConstraint => constraint_anchor(raw),
    }
}

fn constraint_anchor(constraint: &str) -> Option<String> {
    let trimmed = constraint.trim();
    if trimmed.is_empty() || has_compound_range(trimmed) {
        return None;
    }

    let operator = ["===", "==", "~=", ">=", "<=", "!=", "^", "~", ">", "<", "="]
        .into_iter()
        .find(|operator| trimmed.starts_with(operator))
        .unwrap_or("");
    let version = trimmed[operator.len()..].trim();

    if version.is_empty()
        || version.contains('*')
        || version.eq_ignore_ascii_case("latest")
        || version.eq_ignore_ascii_case("next")
        || version.eq_ignore_ascii_case("beta")
    {
        None
    } else {
        Some(version.trim_start_matches('v').to_string())
    }
}

fn is_unconstrained(syntax: RequirementSyntax, requirement: &str) -> bool {
    let trimmed = requirement.trim();
    trimmed.is_empty()
        || trimmed == "*"
        || (syntax == RequirementSyntax::Node
            && matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "latest" | "next" | "beta"
            ))
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = LooseVersion::parse(left);
    let right = LooseVersion::parse(right);
    left.cmp(&right)
}

#[derive(Clone, Debug, Eq)]
struct LooseVersion {
    release: Vec<u64>,
    suffix: String,
}

impl LooseVersion {
    /// Parse only enough structure to consistently order common Python and npm
    /// release strings. This is not a complete PEP 440 or npm semver parser.
    fn parse(value: &str) -> Self {
        let value = value.trim().trim_start_matches('v');
        let value = value
            .rsplit_once('!')
            .map(|(_, rest)| rest)
            .unwrap_or(value);
        let mut release = Vec::new();
        let mut consumed = 0;

        for segment in value.split('.') {
            let digits = segment
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>();
            if digits.is_empty() {
                break;
            }
            release.push(digits.parse().unwrap_or(0));
            consumed += digits.len();
            if consumed < value.len() && value.as_bytes().get(consumed) == Some(&b'.') {
                consumed += 1;
            }
            if digits.len() != segment.len() {
                break;
            }
        }

        while release.last() == Some(&0) {
            release.pop();
        }
        if release.is_empty() {
            release.push(0);
        }

        let suffix = value
            .get(consumed..)
            .unwrap_or_default()
            .to_ascii_lowercase();
        Self { release, suffix }
    }
}

impl PartialEq for LooseVersion {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for LooseVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let length = self.release.len().max(other.release.len());
        for index in 0..length {
            let left = self.release.get(index).copied().unwrap_or(0);
            let right = other.release.get(index).copied().unwrap_or(0);
            match left.cmp(&right) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        }

        match (self.suffix.is_empty(), other.suffix.is_empty()) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => self.suffix.cmp(&other.suffix),
        }
    }
}

impl PartialOrd for LooseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pep508_extras_and_markers() {
        let parsed = parse_pep508("fastapi[standard]>=0.100 ; python_version >= '3.11'").unwrap();
        assert_eq!(parsed.name, "fastapi");
        assert_eq!(parsed.specifier, ">=0.100");
        assert!(!parsed.direct);
    }

    #[test]
    fn recognizes_direct_pep508_dependency() {
        let parsed = parse_pep508("demo @ https://example.com/demo.whl").unwrap();
        assert!(parsed.direct);
    }

    #[test]
    fn rewrites_simple_constraints_without_changing_style() {
        assert_eq!(
            update_single_constraint("^1.2.0", "2.0.0", ConstraintKind::Node).unwrap(),
            "^2.0.0"
        );
        assert_eq!(
            update_single_constraint(">= 1.2", "2.0.0", ConstraintKind::Python).unwrap(),
            ">= 2.0.0"
        );
    }

    #[test]
    fn rejects_compound_and_upper_bound_constraints() {
        assert!(update_single_constraint(">=1,<2", "2.0.0", ConstraintKind::Python).is_err());
        assert!(update_single_constraint("<2", "2.0.0", ConstraintKind::Python).is_err());
    }

    #[test]
    fn compares_release_versions_with_missing_zeroes() {
        assert_eq!(compare_versions("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.2rc1", "1.2"), Ordering::Less);
        assert_eq!(compare_versions("1.10", "1.9"), Ordering::Greater);
    }

    #[test]
    fn extracts_scoped_npm_alias_target() {
        assert_eq!(
            node_registry_target("compat", "npm:@scope/package@^1.0.0").0,
            "@scope/package"
        );
    }
}
