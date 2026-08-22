//! Binary entrypoint.
//!
//! This module wires the CLI orchestration layer to Tokio and translates
//! application errors into the documented process exit code.

mod app;
mod cli;
mod discovery;
mod lockfile;
mod manifest;
mod model;
mod output;
mod registry;
mod version;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match app::run().await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            // Keep error formatting in one place so every command returns the
            // same machine-detectable failure code and a readable cause chain.
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}
