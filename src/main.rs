use anyhow::{Context, Result};
use futures::future::join_all;
use owo_colors::OwoColorize;
use std::time::{Duration, Instant};
use toml_edit::DocumentMut;

#[derive(Debug)]
struct PackageReport {
    name: String,
    required_version: String,
    latest: Result<String, String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let file_path = "pyproject.toml";
    let content = std::fs::read_to_string(file_path)
        .with_context(|| format!("failed to read {}", file_path))?;

    let document = content
        .parse::<DocumentMut>()
        .context("failed to parse pyproject.toml")?;

    let dependencies = document
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
        .context("could not find [project].dependencies array")?;

    let client = reqwest::Client::builder()
        .user_agent("sync/0.1.0")
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .tcp_keepalive(Duration::from_secs(30))
        .pool_max_idle_per_host(1)
        .build()?;

    let start = Instant::now();

    let tasks = dependencies.iter().filter_map(|dep| {
        let raw = dep.as_str()?.to_string();
        let (name, required_version) = split_name_and_version(&raw);
        let client = client.clone();

        Some(async move {
            let latest = fetch_latest(&client, &name)
                .await
                .map_err(|e| e.to_string());
            PackageReport {
                name,
                required_version,
                latest,
            }
        })
    });

    let reports: Vec<PackageReport> = join_all(tasks).await;
    let elapsed = start.elapsed();

    print_table(&reports, elapsed);
    Ok(())
}

fn split_name_and_version(dep: &str) -> (String, String) {
    let without_extras = dep.split('[').next().unwrap_or(dep);

    let stop = ['=', '<', '>', '~', '!', ';', ' '];
    let name_end = without_extras
        .find(|c| stop.contains(&c))
        .unwrap_or(without_extras.len());

    let name = dep[..name_end].trim().to_string();

    let version_part = dep[name_end..].trim();
    let version = if version_part.is_empty() {
        "(any)".to_string()
    } else {
        version_part
            .split(';')
            .next()
            .unwrap_or(version_part)
            .trim()
            .to_string()
    };

    (name, version)
}

async fn fetch_latest(client: &reqwest::Client, name: &str) -> Result<String> {
    let url = format!("https://pypi.org/rss/project/{}/releases.xml", name);
    let body = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to request PyPI releases for {}", name))?
        .error_for_status()
        .with_context(|| format!("PyPI returned an error for {}", name))?
        .text()
        .await?
        .to_string();

    latest_from_release_feed(&body)
        .with_context(|| format!("could not find latest release in PyPI feed for {}", name))
}

fn latest_from_release_feed(feed: &str) -> Option<String> {
    let mut remaining = feed;
    let mut first_release = None;

    while let Some(item_start) = remaining.find("<item>") {
        let item_body = &remaining[item_start..];
        let title_start = item_body.find("<title>")? + "<title>".len();
        let title_body = &item_body[title_start..];
        let title_end = title_body.find("</title>")?;
        let release = decode_xml_entities(title_body[..title_end].trim());

        if first_release.is_none() {
            first_release = Some(release.clone());
        }

        if !is_prerelease_version(&release) {
            return Some(release);
        }

        remaining = &title_body[title_end + "</title>".len()..];
    }

    first_release
}

fn decode_xml_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn is_prerelease_version(version: &str) -> bool {
    let lower = version.to_ascii_lowercase();

    lower.contains("rc")
        || lower.contains("dev")
        || lower.contains("alpha")
        || lower.contains("beta")
        || has_prerelease_letter_marker(&lower, 'a')
        || has_prerelease_letter_marker(&lower, 'b')
}

fn has_prerelease_letter_marker(version: &str, marker: char) -> bool {
    let chars: Vec<char> = version.chars().collect();

    chars.iter().enumerate().any(|(index, char)| {
        if *char != marker {
            return false;
        }

        let previous_is_digit = index
            .checked_sub(1)
            .and_then(|previous| chars.get(previous))
            .is_some_and(|previous| previous.is_ascii_digit());
        let next_is_digit = chars
            .get(index + 1)
            .is_some_and(|next| next.is_ascii_digit());

        previous_is_digit && next_is_digit
    })
}

fn print_table(reports: &[PackageReport], elapsed: Duration) {
    let w_name = reports
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(7)
        .max(7);
    let w_req = reports
        .iter()
        .map(|r| r.required_version.len())
        .max()
        .unwrap_or(8)
        .max(8);
    let w_latest = reports
        .iter()
        .map(|r| r.latest.as_deref().unwrap_or("—").len())
        .max()
        .unwrap_or(6)
        .max(6);

    let total = w_name + w_req + w_latest + 10;

    println!();
    println!("  {}", "─".repeat(total).dimmed());
    println!(
        "  {:<w_name$}  {:<w_req$}  {:<w_latest$}",
        "Package".bold(),
        "Required".bold(),
        "Latest".bold(),
        w_name = w_name,
        w_req = w_req,
        w_latest = w_latest,
    );
    println!("  {}", "─".repeat(total).dimmed());

    for r in reports {
        match &r.latest {
            Ok(latest) => {
                println!(
                    "  {:<w_name$}  {:<w_req$}  {}",
                    r.name.cyan(),
                    r.required_version.dimmed(),
                    latest.green().bold(),
                    w_name = w_name,
                    w_req = w_req,
                );
            }
            Err(err) => {
                println!(
                    "  {:<w_name$}  {:<w_req$}  {}  {}",
                    r.name.cyan(),
                    r.required_version.dimmed(),
                    "—".dimmed(),
                    format!("✗ {}", err).red(),
                    w_name = w_name,
                    w_req = w_req,
                );
            }
        }
    }

    println!("  {}", "─".repeat(total).dimmed());
    println!(
        "  {} packages  ·  {}",
        reports.len().to_string().bold(),
        format!("{:.0?}", elapsed).dimmed()
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_feed_prefers_latest_stable_version() {
        let feed = r#"
            <rss>
              <channel>
                <title>PyPI recent updates for redis</title>
                <item><title>8.0.0b2</title></item>
                <item><title>7.4.0</title></item>
              </channel>
            </rss>
        "#;

        assert_eq!(latest_from_release_feed(feed).as_deref(), Some("7.4.0"));
    }

    #[test]
    fn release_feed_uses_prerelease_when_no_stable_release_exists() {
        let feed = r#"
            <rss>
              <channel>
                <title>PyPI recent updates for example</title>
                <item><title>1.0.0rc2</title></item>
                <item><title>1.0.0b1</title></item>
              </channel>
            </rss>
        "#;

        assert_eq!(latest_from_release_feed(feed).as_deref(), Some("1.0.0rc2"));
    }
}
