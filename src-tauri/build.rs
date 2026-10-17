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
  // The commit the app was built from, for the diagnostic log: where CI
  // built it, which sets `GITHUB_SHA`.
  println!("cargo:rerun-if-env-changed=GITHUB_SHA");
  let commit: String = std::env::var("GITHUB_SHA").map_or_else(
    |_| "local build".to_owned(),
    |sha| sha.chars().take(12).collect(),
  );
  println!("cargo:rustc-env=ARSU_COMMIT={commit}");

  tauri_build::try_build(
    tauri_build::Attributes::new()
      .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
  )
  .expect("failed to generate the Tauri build context");
}
