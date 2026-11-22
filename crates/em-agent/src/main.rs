// SPDX-License-Identifier: Apache-2.0

//! The `em-agent` binary: command line, logging, signals and the runtime
//! (0012 D4, D7, D9). Everything else is in the library.

use std::process::ExitCode;

use em_agent::cli::{self, USAGE};
use em_agent::platform::FakePlatform;
use em_agent::{AgentConfig, run};
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("em-agent: {message}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stdout)
        .with_ansi(false)
        .init();

    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::error!(%error, "cannot install the SIGTERM handler");
            return ExitCode::FAILURE;
        }
    };
    let stop = async {
        tokio::select! {
            _ = terminate.recv() => tracing::info!("SIGTERM received, stopping"),
            _ = tokio::signal::ctrl_c() => tracing::info!("SIGINT received, stopping"),
        }
    };

    tracing::info!(
        control = %args.control.display(),
        data = %args.data.display(),
        profile = args.profile,
        radios = args.radios.len(),
        "em-agent starting"
    );
    let config = AgentConfig {
        control_path: args.control,
        data_path: args.data,
        profile: args.profile,
    };
    match run(config, FakePlatform::new(args.radios), stop).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "em-agent stopped");
            ExitCode::FAILURE
        }
    }
}
