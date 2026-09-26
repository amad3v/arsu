use std::process::ExitCode;

fn main() -> ExitCode {
  match arsu_lib::run() {
    Ok(()) => ExitCode::SUCCESS,
    Err(error) => {
      eprintln!("arsu: {}", cmd::error::describe(&*error));
      ExitCode::FAILURE
    }
  }
}
