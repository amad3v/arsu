# Changelog

Every notable change to Arsu, newest first. A release's notes are its
section here. Versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

### Changed

- On Android, a saved backup is reported the way an import is: the export
  dialog closes and an "Export finished" sheet over the list says the
  backup was saved and that its password must be kept, instead of a
  full-screen page of its own.
- **Reproducible Android builds**, for F-Droid: built from the same commit
  with the same toolchain, the APK is now identical on any machine apart
  from its signature, so F-Droid can check its own build against ours and
  publish ours, with one signature everywhere.
  - The Rust library no longer embeds the build machine's Cargo and rustup
    paths (in panic locations): the Gradle task that builds it remaps
    them (`--remap-path-prefix`) to `/cargo` and `/rustup`.
  - The stylesheet no longer changes from one build to the next: UnoCSS
    emitted the theme's CSS variables in the order the build happened to
    meet them, and now emits all of them in the theme's order (about 4 KB
    more, compressed).
  - The release workflow rebuilds the APK a second time, independently
    (another directory, other Cargo and rustup homes, no cache), and
    publishes nothing unless both builds match apart from the signature
    (`apksigcopier compare`); `.github/scripts/apk-diff.py` lists the
    entries that differ. It builds with an exact Node version (26.10.0),
    as F-Droid's recipe does.
  - The Android version code is written in `src-tauri/tauri.android.conf.json`
    for F-Droid to read; the release workflow checks it is the one Tauri
    derives from the version (major × 1000000 + minor × 1000 + patch).
- F-Droid's listing (name, summary, description, icon) lives in
  `fastlane/metadata/android/en-US`.

## [1.1.0] — 2026-10-05

### Added

- **Android.** Arsu builds for Android as well as the Linux desktop, from
  the same Rust core and interface.
  - Platform calls through JNI: the clipboard (codes marked sensitive, so
    Android's clipboard preview and keyboards hide them), opening links,
    and reading or writing documents picked in the system's file picker
    (`content://` URIs). An unreadable clipboard (Android only lets the
    app in the foreground read it) is cleared after the delay rather than
    left holding the code.
  - The vault and settings live in the app's private data directory.
  - The vault locks within a second of the screen going off or the lock
    screen showing, whatever the auto-lock delay.
  - The Android project ships without the INTERNET permission outside
    debug builds, opts out of cloud backup, blocks screenshots and screen
    recording (`FLAG_SECURE`), and keeps clear of the system bars and the
    keyboard in edge-to-edge mode.
- **A touch interface**, chosen at start-up when running on Android, in
  the same look as the desktop: large entry rows (a tap copies the code,
  the ⋮ opens the rarer actions in a bottom sheet), an app bar with
  search, lock and settings, a + button for adding, importing and
  exporting, settings as a page of grouped rows, every dialog full-screen,
  and Back closing whatever is open on top before it leaves the app.
- **Fingerprint and face unlock** (strong, class 3 biometrics) on Android.
  The vault key is sealed under an Android Keystore key that the secure
  hardware uses only after the biometric check passes; adding a
  fingerprint or face to the phone destroys it. Turning it on asks for
  the master password again. Showing an entry's QR code can use it
  instead of the password too.
- **A warning on rooted phones.** At start-up, a phone where `su` is found
  shows the risk (an app with root can read the vault and the codes) and
  asks for consent before the vault opens; declining closes the app. The
  check is best effort: root hidden from apps goes unseen.
- The first-start screen warns that uninstalling Arsu, or clearing its
  storage, deletes the vault, and points to the export.
- Releases are built by GitHub Actions: the Linux binary, `.deb` and
  `.rpm`, and the Android APK signed with the release key, published with
  their notes from this file and a `SHA256SUMS`. CI also lints the Android
  code.

### Changed

- Import formats are told apart by content, not by file name: an Aegis
  vault export or a 2FAS backup imports under any name, and anything else
  is refused whatever its name. The Linux dialog also offers "All files".
- Backups fall back to a crash-safe copy where hard links are refused, as
  Android's SELinux policy refuses them to apps.
- The About dialog names the web engine the app runs in, and shows no
  file paths on Android, where they are private to the app.
- Dependencies updated: `jni` 0.22 (the Android calls ported to its
  safer API: thread attachment through a closure, which also catches Java
  exceptions, and names and signatures checked at compile time),
  `tauri-plugin-single-instance` 2.5, `thiserror` 2.0.21, `uuid` 1.27.
  Tauri itself still uses `jni` 0.21, so both are built until it moves.

### Fixed

- On Android, the fingerprint prompt now always shows when the phone is
  unlocked again with Arsu locked. The system cancels a prompt as the
  screen goes off without saying so, and the next ones were then ignored;
  Arsu now withdraws its own prompt as it leaves the foreground and shows
  a fresh one once it is back and focused.
- The lock screen keeps the password usable while a fingerprint prompt is
  pending: the Unlock button no longer stays stuck on "Unlocking…".
- On a phone, the About page fits the screen and its licence scrolls in
  its own box.

## [1.0.1]

### Changed

- The window shows only once the interface has rendered, so no blank
  page flashes at launch.
- Built with Tauri 2.12, which lets Wayland compositors that draw window
  frames (KWin) draw Arsu's.

## [1.0.0]

- First release: an offline TOTP/HOTP authenticator for Linux with an
  encrypted local vault.
