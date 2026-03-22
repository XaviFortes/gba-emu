mod app;

use std::process::ExitCode;

use app::cli::parse_args;

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(code) => return code,
    };

    app::runner::run(args)
}
