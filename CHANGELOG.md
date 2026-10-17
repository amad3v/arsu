# Changelog

Every notable change to Arsu, newest first. A release's notes are its
section here. Versions follow [Semantic Versioning](https://semver.org).

## [Unreleased]

### Added

- **Sync between your devices, without any network.** The new Sync
  screen (the header's Sync button on the desktop; Settings → Sync with
  your devices on Android) pairs a device once — it shows a QR code and
  a code to type — and then keeps both vaults the same, over Bluetooth or
  with an encrypted sync file you move yourself.
  - Accounts added on one device appear on the others, deletions follow,
    HOTP counters never go back, and an account added on two devices
    ends up once. Sessions are end-to-end encrypted (the Noise protocol);
    each device keeps its own master password.
  - Any two of your devices sync over Bluetooth: phones only, computers
    only, or a mix. One chooses **Wait for my devices**, the other **Sync
    with a device nearby**; a search that finds one device waiting syncs
    with it at once, and lists them when there are several.
  - Pairing over Bluetooth works the same between any two devices: the
    inviting one waits, the other scans its QR code (a computer types
    its code). Phones pair with each other directly, without a join
    file.
  - A new device joins through any one of yours, and the others learn of
    it at their next sync with a device that knows it: a computer added
    to your phones pairs with one of them, and syncs with each other
    phone once that phone has synced with the first. Until then, a phone
    refuses it and says how to fix it.
  - A phone whose Bluetooth cannot advertise says so when asked to wait;
    it still finds and joins the others, and its invitation's QR code
    still makes a join file.
  - Removing a device stops it from syncing, but it keeps what it
    already has. Syncing waits for a paired device, and the Sync screen
    says so. A phone is named after its model; two devices listed under
    one name (as reinstalling Arsu leaves: it makes a new device) are
    pointed out, to remove the earlier one.
- **A diagnostic report for bug reports**, on every device. When
  something goes wrong, **About → Save diagnostic report** shows
  everything the report holds, then saves it as one zip file to attach
  to an issue, which says why.
  - The log is JSON Lines: one self-contained JSON object per line, so a
    crash or a cut-off upload never spoils the rest, and it is easy to
    search. Every line is a failure: its time (RFC 3339, UTC, to the
    millisecond), its level, the module, source file and line that
    recorded it, a short constant message, the error's kind and every
    cause, its specifics as structured fields (a Bluetooth link's role, a
    scan's counts), the app run it belongs to (a session id, new at each
    launch) and its place in that run (a sequence number, which keeps the
    order if the clock is set back).
  - It records warnings, errors and critical failures (crashes, a vault
    that cannot be read or saved, no randomness), nothing routine. A
    failure carries the steps that led to it (the commands called, by
    name only, and what Bluetooth and sync did in the five minutes
    before, 30 at most), kept in memory until then, and each step goes
    with one failure only; a step repeated is counted rather than
    repeated. Polling (code refreshes, the auto-lock's heartbeat) is no
    step. Failures you are told about anyway, such as a wrong password or
    a mistyped code, are not logged.
  - Every failure the app reports goes in, from any screen; so do
    crashes (with their location, and a backtrace where the build has
    one), errors in the interface itself (its error's name, message and
    where it happened), and each Bluetooth scan and session that fails,
    with how far the connection got.
  - An app run's failures follow a record of what the app and the device
    were: Arsu's version, build and commit, the operating system and its
    version (the kernel's on Linux, the API level on Android), the
    phone's maker and model, the processor, the language and region, the
    WebView and Tauri versions; an update between two runs shows. A run
    with no failure writes nothing. No host name, user name or path.
  - The report holds the log files, oldest first, and an
    `environment.json`: the device and app as above, and the state the
    app was in (whether the vault exists and is unlocked, how many
    devices it trusts, whether biometric unlock is on, the settings).
    You see it all (the environment and the latest 500 records, or that
    nothing has gone wrong) before choosing to save it; cancelling saves
    nothing.
  - It never holds an account, a secret, a code, an invitation, a
    password or a device's name; whatever looks like one, your home
    directory (wherever it is) or a Bluetooth address (all but its last
    two bytes) is masked as each record is written, so nothing unmasked
    ever reaches the disk.
  - The log stays in the app's private log directory, rotated at 1 MB
    with five files kept (5 MB at most), and never leaves the device
    unless you save a report. Earlier text logs are deleted.
- **A QR code scanner on Android.** Point the camera at an account's QR
  code (**Scan QR code** in the + sheet) or at a sync invitation (**Scan
  QR** when joining). Arsu asks for the camera the first time you scan,
  and uses it only while the scanner is open. Joining then asks, in a
  sheet, whether to pair over Bluetooth or save a join file, as soon as
  the code is read; errors appear in a sheet too.
- On Android, Bluetooth sync asks for the **Nearby devices** permission
  the first time a phone looks for your devices or waits for them
  (Android 8 to 11 ask for location instead, which they tie to Bluetooth
  scanning; Location must then be on to find a device). Never at launch, and nothing else: the
  release build is checked for any other permission, the network above
  all.

### Security

- **Deleting an account now erases its secret.** A deleted account used
  to stay in the encrypted vault, secret, name and notes included, only
  marked as deleted. Now only its id and the time of deletion remain,
  which a future sync needs to keep it deleted on every device. Vaults
  written by earlier versions are cleaned the next time they are saved.

### Fixed

- A dialog opened from inside another one (an error shown over the Sync
  screen, for one) is no longer hidden from screen readers: a closed
  dialog leaves nothing in the page for an open one to hide.
- The app no longer zooms when pinched: on a phone or a touchscreen, or
  with two fingers on a touchpad (Android also zoomed on a
  double tap).

## [1.1.3] — 2026-10-06

### Changed

- On Android, every dialog but About is now a bottom sheet instead of a
  full-screen page: Delete, Show QR code, Add entry, Import entries,
  Export encrypted backup, and turning on fingerprint or face unlock. A
  sheet opened from another (an entry's actions, the + button) or a
  result after a dialog replaces it: the first slides away, then the next
  rises, rather than one covering the other.
- Arsu now **requires Android 8.0** (API 26), with an up-to-date Android
  System WebView (version 111 or later; tested with 138, the last one
  Android 8 and 9 get). The APK said Android 7.0, where it crashed at
  launch: Tauri's Android library needs API 26. On an older WebView the
  app shows a blank screen.

## [1.1.2] — 2026-10-06

### Fixed

- The Android APK no longer carries the Android Gradle plugin's
  "Dependency metadata" signing block (`dependenciesInfo` disabled), which
  F-Droid refuses: it is encrypted with Google's key and can't be checked.

## [1.1.1] — 2026-10-06

### Changed

- On the desktop, the theme is chosen in the settings menu, under
  **Select theme** (Light, Dark or System), set apart from the vault's
  settings above it and About below. The lock and create-vault screens
  no longer offer it: nothing on them but what unlocks. A new install
  follows the system's theme, as before.
- The desktop's entry list is headed as Android's is: the app's logo,
  then "Arsu" and the entry count.
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
  `fastlane/metadata/android/en-US`, with each version's "what's new" in
  its `changelogs/`.
- Android's own Java and Kotlin code (the app and the biometric plugin)
  targets Java 17 instead of 8, and is configured through the Android
  Gradle plugin's DSL interfaces: our build files no longer raise Gradle
  deprecation warnings.

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
