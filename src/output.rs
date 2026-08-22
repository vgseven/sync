//! Human-readable table and machine-readable JSON report rendering.
//!
//! Both output formats are derived from the same `ReportRow` list so automation
//! sees the same status decisions that a terminal user sees.

use crate::cli::OutputFormat;
use crate::model::{ReportRow, ReportStatus};
use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct JsonReport<'a> {
    summary: Summary,
    dependencies: &'a [ReportRow],
}

#[derive(Clone, Copy, Serialize)]
pub struct Summary {
    pub total: usize,
    pub current: usize,
    pub outdated: usize,
    pub ahead: usize,
    pub unconstrained: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl Summary {
    /// Count each terminal status once for exit-code and report consumers.
    pub fn from_rows(rows: &[ReportRow]) -> Self {
        let mut summary = Self {
            total: rows.len(),
            current: 0,
            outdated: 0,
            ahead: 0,
            unconstrained: 0,
            skipped: 0,
            errors: 0,
        };

        for row in rows {
            match row.status {
                ReportStatus::Current => summary.current += 1,
                ReportStatus::Outdated => summary.outdated += 1,
                ReportStatus::Ahead => summary.ahead += 1,
                ReportStatus::Unconstrained => summary.unconstrained += 1,
                ReportStatus::Skipped => summary.skipped += 1,
                ReportStatus::Error => summary.errors += 1,
            }
        }

        summary
    }
}

pub fn print(rows: &[ReportRow], format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => print_json(rows),
        OutputFormat::Table => {
            print_table(rows);
            Ok(())
        }
    }
}

fn print_json(rows: &[ReportRow]) -> Result<()> {
    let report = JsonReport {
        summary: Summary::from_rows(rows),
        dependencies: rows,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn print_table(rows: &[ReportRow]) {
    if rows.is_empty() {
        println!("No dependencies found.");
        return;
    }

    println!(
        "{:<8}  {:<28}  {:<24}  {:<24}  {:<16}  {:<16}  STATUS",
        "ECO", "MANIFEST", "GROUP", "PACKAGE", "CURRENT", "LATEST"
    );
    println!("{}", "-".repeat(134));

    for row in rows {
        println!(
            "{:<8}  {:<28}  {:<24}  {:<24}  {:<16}  {:<16}  {}{}",
            row.ecosystem,
            truncate(&row.manifest, 28),
            truncate(&row.group, 24),
            truncate(&row.package, 24),
            truncate(&row.current, 16),
            truncate(row.latest.as_deref().unwrap_or("-"), 16),
            row.status,
            row.message
                .as_ref()
                .map(|message| format!(" ({message})"))
                .unwrap_or_default()
        );
    }

    let summary = Summary::from_rows(rows);
    println!("{}", "-".repeat(134));
    println!(
        "{} dependencies, {} outdated, {} current, {} skipped, {} errors",
        summary.total, summary.outdated, summary.current, summary.skipped, summary.errors
    );
}

fn truncate(value: &str, max: usize) -> String {
    // Count characters rather than bytes so table truncation never splits UTF-8.
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut output = value
        .chars()
        .take(max.saturating_sub(3))
        .collect::<String>();
    output.push_str("...");
    output
}
