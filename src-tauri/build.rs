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

/// Runs `git` with `args` in the package's directory, for its trimmed
/// output; `None` when git is missing or fails (a source tarball).
fn git(args: &[&str]) -> Option<String> {
  let out = std::process::Command::new("git").args(args).output().ok()?;
  out
    .status
    .success()
    .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
  // The commit the app was built from, for the diagnostic log. It comes
  // from the checkout itself, so that every build of one commit embeds
  // the same value: F-Droid rebuilds the APK outside GitHub and must get
  // the same bytes. `GITHUB_SHA` is the same commit where CI built it.
  println!("cargo:rerun-if-env-changed=GITHUB_SHA");
  if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
    // A new commit moves the branch's ref, not HEAD itself. Only files
    // that exist: cargo reruns the script every build for a missing one.
    let head_ref = git(&["symbolic-ref", "-q", "HEAD"]);
    let watched = ["HEAD", "packed-refs"]
      .into_iter()
      .chain(head_ref.as_deref())
      .map(|path| format!("{git_dir}/{path}"));
    for path in watched.filter(|path| std::path::Path::new(path).exists()) {
      println!("cargo:rerun-if-changed={path}");
    }
  }
  let commit: String = std::env::var("GITHUB_SHA")
    .ok()
    .or_else(|| git(&["rev-parse", "HEAD"]))
    .map_or_else(
      || "local build".to_owned(),
      |sha| sha.chars().take(12).collect(),
    );
  println!("cargo:rustc-env=ARSU_COMMIT={commit}");

  tauri_build::try_build(
    tauri_build::Attributes::new()
      .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
  )
  .expect("failed to generate the Tauri build context");
}
