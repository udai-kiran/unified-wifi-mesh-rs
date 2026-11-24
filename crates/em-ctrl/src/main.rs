// SPDX-License-Identifier: Apache-2.0

//! The `em-ctrl` binary: command line, logging, signals and the runtime
//! (0012 D7, D9). Everything else is in the library.

use std::process::ExitCode;

use em_ctrl::cli::{self, USAGE};
use em_ctrl::{CtrlConfig, run};
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("em-ctrl: {message}");
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
        reset_file = %args.reset_file.display(),
        drop_first_m2 = args.drop_first_m2,
        "em-ctrl starting"
    );
    // intentional-difference:reset-file-required
    // Upstream logs a missing file and continues with no SSIDs
    // (`dm_easy_mesh_ctrl.cpp:10855-10858`), after which it can onboard nothing (0013 D2).
    let networks = match em_db::load_reset_file(&args.reset_file) {
        Ok(networks) => networks,
        Err(error) => {
            tracing::error!(%error, "the reset file is required, em-ctrl cannot start");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(rows = networks.len(), "reset file loaded");
    if args.drop_first_m2 {
        tracing::warn!("test switch --test-drop-first-m2 is on: the first M2 will not be sent");
    }
    let config = CtrlConfig {
        control_path: args.control,
        data_path: args.data,
        networks,
        drop_first_m2: args.drop_first_m2,
    };
    match run(config, stop).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "em-ctrl stopped");
            ExitCode::FAILURE
        }
    }
}
