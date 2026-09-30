//! `auto-post` command line interface.
//!
//! * [`args`]: argument parsing (clap) and `KEY=VALUE` parsing.
//! * [`commands`]: `validate` and `run` handlers, independent of the process
//!   environment (output sink and HTTP client are injected).
//! * [`logging`]: verbose request logging with secret masking.
//!
//! Exit codes: 0 success, 1 validation/run failure, 2 usage error.

pub mod args;
pub mod commands;
pub mod error;
pub mod logging;

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use auto_post_core::http::{HttpClient, ReqwestClient};
use clap::error::ErrorKind;
use clap::Parser;

use args::{Cli, Command};
use logging::LoggingClient;

/// Process exit codes.
pub const EXIT_OK: u8 = 0;
pub const EXIT_FAILURE: u8 = 1;
pub const EXIT_USAGE: u8 = 2;

/// Parses `args`, executes the command with the real HTTP client and returns
/// the process exit code.
pub async fn main_exit<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let code = match error.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => EXIT_OK,
                _ => EXIT_USAGE,
            };
            let _ = error.print();
            return ExitCode::from(code);
        }
    };
    let mut stdout = std::io::stdout();
    match dispatch(cli, &mut stdout).await {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

async fn dispatch(cli: Cli, out: &mut dyn Write) -> Result<(), error::CliError> {
    match cli.command {
        Command::Validate(args) => commands::validate(&args, out),
        Command::Run(args) => {
            let client = ReqwestClient::default();
            if args.verbose {
                let logging = LoggingClient::new(client, |line: &str| eprintln!("{line}"));
                commands::run(&args, &logging as &dyn HttpClient, out).await
            } else {
                commands::run(&args, &client as &dyn HttpClient, out).await
            }
        }
    }
}
