//! The app shell: hardens the process, sets up the Tauri plugins, and
//! registers the state and commands of the `cmd` crate, where the IPC
//! surface lives.
//!
//! Security-relevant configuration lives next to this file:
//! `tauri.conf.json` sets the Content Security Policy (no remote origin
//! of any kind), and `capabilities/default.json` grants the window
//! exactly the app's own commands (`permissions/app-commands.toml`) and
//! event listening — no plugin commands. The file dialogs (GTK's) and the
//! clipboard are driven from Rust, which the capability system does not
//! gate, so the `WebView` itself can open neither.

#[macro_use]
mod app_commands;

use std::{error::Error, thread, time::Duration};

use tauri::{AppHandle, Manager, RunEvent};

use cmd::{
  AppState, auto_lock,
  clipboard::{self, ClipboardExt},
  commands,
};

macro_rules! invoke_handler {
  ($($command:ident),* $(,)?) => {
    tauri::generate_handler![$(commands::$command),*]
  };
}

/// Runs the app until its window closes. A second launch focuses the
/// running instance's window instead, and exits.
///
/// # Errors
///
/// Returns an error if the process cannot be hardened, the data
/// directories cannot be determined, or Tauri fails.
pub fn run() -> Result<(), Box<dyn Error>> {
  #[cfg(target_os = "linux")]
  harden_process()?;

  let app = tauri::Builder::default()
    // First, so that a second instance hands over before anything else
    // starts; the vault's own lock backs this up (`VaultInUse`).
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      focus_main_window(app);
    }))
    .manage(clipboard::Clipboard::new())
    .setup(|app| {
      app.manage(AppState::new(&cmd::app_paths()?));
      auto_lock::spawn(app.handle().clone())?;
      show_main_window_eventually(app.handle().clone());
      Ok(())
    })
    .invoke_handler(with_app_commands!(invoke_handler))
    .build(tauri::generate_context!())?;

  app.run(|app_handle, event| {
    // The clipboard plugin used to do this itself on exit; now that the
    // clipboard is owned directly (see `cmd::clipboard`), this is the
    // one place left to release it — Tauri does not drop managed state.
    if let RunEvent::Exit = event {
      clipboard::shut_down(
        app_handle.clipboard(),
        app_handle.state::<AppState>().copied_codes(),
      );
    }
  });
  Ok(())
}

/// How long the window may stay hidden at launch: the frontend shows it
/// (`show_window`) as soon as it has rendered, which takes well under
/// this, and this is the fallback if it never does, so that a broken
/// frontend still leaves a window to see its error in.
const SHOW_WINDOW_FALLBACK: Duration = Duration::from_secs(3);

fn show_main_window_eventually(app: AppHandle) {
  thread::spawn(move || {
    thread::sleep(SHOW_WINDOW_FALLBACK);
    if let Some(window) = app.get_webview_window("main")
      && !window.is_visible().unwrap_or(false)
    {
      let _ = window.show();
    }
  });
}

fn focus_main_window(app: &AppHandle) {
  if let Some(window) = app.get_webview_window("main") {
    // Best effort: if the window manager refuses, the running window
    // simply stays where it is.
    let _ = window
      .unminimize()
      .and_then(|()| window.show())
      .and_then(|()| window.set_focus());
  }
}

/// Keeps the decrypted vault out of core dumps and away from other
/// processes:
/// - `PR_SET_DUMPABLE = 0`: a crash writes no core dump — which
///   systemd-coredump would otherwise store, key and seeds included, on
///   disk — and processes of the same user can neither `ptrace` this one
///   nor read its memory through `/proc`.
/// - `RLIMIT_CORE = 0`: no core file either way.
///
/// It applies to this process only: the `WebView` runs in child
/// processes, which never hold the key.
#[cfg(target_os = "linux")]
fn harden_process() -> std::io::Result<()> {
  use rustix::process::{DumpableBehavior, Resource, Rlimit, set_dumpable_behavior, setrlimit};

  setrlimit(
    Resource::Core,
    Rlimit {
      current: Some(0),
      maximum: Some(0),
    },
  )?;
  set_dumpable_behavior(DumpableBehavior::NotDumpable)?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  macro_rules! command_names {
    ($($command:ident),* $(,)?) => {
      [$(stringify!($command)),*]
    };
  }

  #[test]
  fn the_capability_grants_exactly_the_registered_commands() {
    let permission_set = include_str!("../permissions/app-commands.toml");
    let mut granted: Vec<&str> = permission_set
      .lines()
      .filter_map(|line| line.trim().strip_prefix("\"allow-"))
      .filter_map(|rest| rest.strip_suffix("\","))
      .collect();
    let mut registered: Vec<String> = with_app_commands!(command_names)
      .iter()
      .map(|command| command.replace('_', "-"))
      .collect();
    granted.sort_unstable();
    registered.sort_unstable();

    assert_eq!(granted, registered);
  }

  #[test]
  fn the_window_has_no_devtools_and_a_minimum_size() {
    let config: serde_json::Value =
      serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let window = &config["app"]["windows"][0];
    // Off in debug builds too; release builds would also need Tauri's
    // `devtools` feature, which the workspace doesn't enable.
    assert_eq!(window["devtools"], false);
    let workspace = include_str!("../../Cargo.toml");
    let tauri = workspace
      .lines()
      .find(|line| line.starts_with("tauri ="))
      .unwrap();
    assert!(!tauri.contains("devtools"), "{tauri}");

    assert_eq!(
      (&window["minWidth"], &window["minHeight"]),
      (&860.into(), &800.into())
    );
  }

  #[test]
  fn the_window_starts_hidden_until_the_frontend_shows_it() {
    let config: serde_json::Value =
      serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();

    assert_eq!(config["app"]["windows"][0]["visible"], false);
  }

  #[cfg(target_os = "linux")]
  #[test]
  fn the_process_cannot_dump_core() {
    use rustix::process::{DumpableBehavior, Resource, dumpable_behavior, getrlimit};

    harden_process().unwrap();

    assert_eq!(dumpable_behavior().unwrap(), DumpableBehavior::NotDumpable);
    let core = getrlimit(Resource::Core);
    assert_eq!((core.current, core.maximum), (Some(0), Some(0)));
  }
}
