//! Crash-consistency test for the vault save path.
//!
//! This lives in its own test binary on purpose and must stay the only
//! test here. It forks a child process, and a fork copies every open
//! file descriptor, including the `<vault>.lock` locks held by sibling
//! tests. Those would stay locked until the child execs, making
//! unrelated tests fail with `InUse` when they re-open a vault.

use std::{
  env, fs,
  os::unix::ffi::OsStrExt,
  path::{Path, PathBuf},
  process::Command,
};

use crypto::KdfParams;
use otp::Algorithm;
use storage::VaultStorage;
use uuid::Uuid;
use vault_core::{Entry, OtpConfig};

const PASSWORD: &[u8] = b"correct horse battery staple";
const CRASH_CHILD_ENV: &str = "ARSU_STORAGE_CRASH_TEST_VAULT";
const CRASH_TEST_NAME: &str = "a_save_killed_mid_write_leaves_the_vault_intact";

fn storage(path: &Path) -> VaultStorage {
  VaultStorage::new(path.to_path_buf())
    .unwrap()
    .with_kdf_params(KdfParams::MINIMUM)
}

fn entry(label: &str) -> Entry {
  Entry {
    id: Uuid::now_v7(),
    issuer: Some("Example".to_string()),
    account_label: label.to_string(),
    otp: OtpConfig::Totp {
      secret: b"12345678901234567890".to_vec().into(),
      algorithm: Algorithm::Sha1,
      digits: 6,
      period: 30,
    },
    icon: None,
    tags: vec![],
    notes: None,
    created_at: 0,
    updated_at: 0,
    deleted_at: None,
  }
}

fn temp_files(dir: &Path) -> Vec<PathBuf> {
  fs::read_dir(dir)
    .unwrap()
    .map(|e| e.unwrap().path())
    .filter(|p| p.file_name().unwrap().as_bytes().starts_with(b".vault-"))
    .collect()
}

/// The real failure: this test re-runs itself in a child process whose
/// file-size limit (`ulimit -f`) is far below the size of the vault it
/// saves, so the kernel kills it with `SIGXFSZ` in the middle of
/// writing. The vault and its backups must be exactly as they were —
/// the scenario that used to leave no vault at all, because the old
/// save renamed the vault away before writing. `ulimit -c 0` stops the
/// killed child from leaving a core dump behind on every run.
#[test]
fn a_save_killed_mid_write_leaves_the_vault_intact() {
  if let Some(path) = env::var_os(CRASH_CHILD_ENV) {
    return save_until_killed(Path::new(&path));
  }

  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("vault");
  {
    let storage = storage(&path);
    let mut vault = storage.create(PASSWORD).unwrap();
    vault.payload.entries.push(entry("before the crash"));
    storage.save(&vault).unwrap();
  } // releases the lock for the child
  let vault_before = fs::read(&path).unwrap();
  let backup_before = fs::read(path.with_added_extension("bak.1")).unwrap();

  let child = Command::new("sh")
    .args(["-c", r#"ulimit -c 0 && ulimit -f 64 && exec "$0" "$@""#])
    .arg(env::current_exe().unwrap())
    .args(["--exact", CRASH_TEST_NAME, "--nocapture"])
    .env(CRASH_CHILD_ENV, &path)
    .output()
    .unwrap();

  assert!(
    child.status.code().is_none(),
    "the child must be killed mid-write, got {}:\n{}",
    child.status,
    String::from_utf8_lossy(&child.stderr)
  );
  assert_eq!(
    temp_files(dir.path()).len(),
    1,
    "the interrupted write left its temp file"
  );
  assert_eq!(fs::read(&path).unwrap(), vault_before);
  assert_eq!(
    fs::read(path.with_added_extension("bak.1")).unwrap(),
    backup_before,
    "no backup slot was consumed"
  );
  assert!(!path.with_added_extension("bak.2").exists());

  let storage = storage(&path);
  assert!(
    temp_files(dir.path()).is_empty(),
    "stale temp file cleaned up"
  );
  let vault = storage.open(PASSWORD).unwrap();
  let labels: Vec<&str> = vault
    .payload
    .entries
    .iter()
    .map(|e| e.account_label.as_str())
    .collect();
  assert_eq!(labels, ["before the crash"]);
}

/// Child side of the test above.
fn save_until_killed(path: &Path) {
  let storage = storage(path);
  let mut vault = storage.open(PASSWORD).unwrap();
  vault.payload.entries.push(Entry {
    notes: Some("x".repeat(1 << 20)),
    ..entry("oversized")
  });
  let result = storage.save(&vault);
  panic!("the file-size limit should have killed this save, but it returned {result:?}");
}
