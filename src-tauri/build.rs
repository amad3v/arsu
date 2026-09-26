include!("src/app_commands.rs");

macro_rules! command_names {
  ($($command:ident),* $(,)?) => {
    &[$(stringify!($command)),*]
  };
}

/// Every app command gets generated `allow-`/`deny-` permissions, and
/// with an app manifest in place Tauri refuses any command that no
/// capability grants.
const APP_COMMANDS: &[&str] = with_app_commands!(command_names);

fn main() {
  tauri_build::try_build(
    tauri_build::Attributes::new()
      .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
  )
  .expect("failed to generate the Tauri build context");
}
