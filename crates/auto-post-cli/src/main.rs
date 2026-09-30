//! Thin entry point: everything testable lives in the library.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    auto_post_cli::main_exit(std::env::args_os()).await
}
