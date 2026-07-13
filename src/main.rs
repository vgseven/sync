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
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}
