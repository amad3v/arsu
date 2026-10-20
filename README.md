# Arsu

**Offline TOTP/HOTP authenticator with an encrypted local vault**, for the Linux desktop and Android.

Arsu keeps your two-factor secrets in a single encrypted file on your machine and shows their codes. It never connects to the network: no account, no cloud, no telemetry. Your devices sync directly with each other, over Bluetooth or through a file you move yourself.

<!-- Screenshot: add .github/screenshot.png and uncomment.
![Arsu's main window](.github/screenshot.png)
-->

Named after Arsu, the god of the evening star worshipped at Palmyra, who protected the caravans crossing the desert.

## Features

- **Encrypted vault.** Your master password is stretched with Argon2id (256 MiB, 3 passes), and the whole vault is encrypted with XChaCha20-Poly1305. The file is useless without the password.
- **Crash-safe saves** that never leave a half-written vault, plus the last three versions kept next to it, in case of a bad edit.
- **Add accounts** by scanning their QR code with the camera (Android), from an `otpauth://` link, a screenshot of a QR code (paste it, drop it, or pick the file), or by hand.
- **TOTP and HOTP**, SHA-1/256/512, 6–8 digits, with the next code shown before the current one expires.
- **Quick to use from the keyboard:** type to search, arrows to choose, Enter to copy. Or double-click a row.
- **Locks itself** after an idle time you choose (5 minutes by default, counting time the computer sleeps).
- **Clears copied codes** from the clipboard after a delay you choose (20 seconds by default), unless you copied something else meanwhile, and asks clipboard managers not to keep them in their history. On Android, codes are marked sensitive, so the clipboard preview and keyboards don't show them.
- **Imports** Aegis and 2FAS backups, plain or encrypted, skipping accounts you already have. **Exports** an encrypted Aegis backup, or a single account as a QR code (after asking for your master password again).
- **Light and dark themes**, or following the system.
- **Syncs your devices with each other, offline.** Pair a device once, by QR code or a typed code, then sync over Bluetooth, between any two of your phones and computers, or with an encrypted sync file. Each device keeps its own master password; nothing is ever sent over a network.

## Install

### From a release

Download the package for your distribution from the [Releases page](https://github.com/amad3v/arsu/releases), along with `SHA256SUMS` and `SHA256SUMS.asc`. Check them, then install:

```sh
curl -sL https://github.com/amad3v.gpg | gpg --import   # the signing key, once
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS

# Debian, Ubuntu and derivatives
sudo apt install ./arsu_1.2.1_amd64.deb

# Fedora, openSUSE and other RPM-based distributions
sudo dnf install ./arsu-1.2.1-1.x86_64.rpm
```

`gpg --verify` must report a good signature from Mohamed Jouini <amad3v@gmail.com>, key fingerprint `6A70 0E00 3968 20D9 3A82  9FF9 1CAC 141C 3451 6CB6`.

### Arch Linux

Arsu is on the AUR in two packages, both checked against the signing key above:

- [`arsu`](https://aur.archlinux.org/packages/arsu) builds it from the signed release tag.
- [`arsu-bin`](https://aur.archlinux.org/packages/arsu-bin) installs the released binary.

```sh
yay -S arsu   # or arsu-bin; any AUR helper, or git clone + makepkg
```

### Android

Each [release](https://github.com/amad3v/arsu/releases) has the APK, `arsu-<version>.apk`; or build it from source (see [Building for Android](#building-for-android)). It needs Android 8.0 (API 26) or later, with an up-to-date Android System WebView (version 111 or later; tested with 138, the last one Android 8 and 9 get).

## Syncing your devices

Arsu keeps one vault the same on all your phones and computers without any network: devices talk to each other directly over Bluetooth, or through a sync file you move yourself. Accounts added on one device appear on the others, deleted ones are deleted everywhere, HOTP counters never go back, and an account added on two devices ends up once. Each device keeps its own master password.

Sync is optional: a single device needs none of it.

### What you need

- **Bluetooth LE** on both devices, and both vaults unlocked.
- **On Linux:** BlueZ (the `bluez` package) with the Bluetooth service running (`systemctl enable --now bluetooth`). To wait for other devices, the adapter must be able to advertise, as nearly every built-in adapter and recent USB dongle can.
- **On Android:** the **Nearby devices** permission (Android 12 and later), or location (Android 8 to 11, which tie Bluetooth scanning to it; Location must then be on to find a device). Arsu asks the first time it needs it. A phone whose Bluetooth can't advertise still finds and joins the others; it just can't be the one that waits.
- **No Bluetooth?** Sync files do the same job; see below.

### Pairing a device, once

Open **Sync**: the header's Sync button on a computer, **Settings → Sync with your devices** on a phone.

1. On a device that already has your vault, choose **Pair a new device**. It shows an invitation, as a QR code and a code to type, good for one pairing within 15 minutes, and waits.
2. On the new device, choose **Join with an invitation**. A phone scans the QR code (**Scan QR**); a computer types the code, or opens a picture of the QR code (**Open image**).
3. Confirm **Pair over Bluetooth**. Both devices then have the accounts of both vaults.

A new device only needs to pair with one of yours: the others learn of it at their next sync with a device that knows it.

### Syncing

On one device choose **Wait for my devices**, on the other **Sync with a device nearby**. If it finds one device waiting, it syncs with it at once; if several, it lists them. A device that waits keeps serving your devices one after another until you leave the screen. Locking the vault stops it.

### Without Bluetooth: join files and sync files

- **To pair:** after scanning the invitation's QR code, choose **Save a join file instead**, open that file on the device that showed the invitation, and open the sync file it gives back on the new device (**Open a sync file**).
- **To sync:** choose **Sync file** next to a device to save a file for it, then open it on that device with **Open a sync file**.

A sync file (`.arsu`) is encrypted for the one device it is meant for, so it is safe to move over any channel.

### Removing a device

**Remove** next to a device stops it from syncing with your vault; it keeps the accounts it already has. A reinstalled Arsu is a new device: when two devices share a name, the Sync screen points it out, so you can remove the old one.

## Your data

On Linux:

| What                  | Where                                                                                    |
| --------------------- | ---------------------------------------------------------------------------------------- |
| Vault                 | `$XDG_DATA_HOME/arsu/vault` (`~/.local/share/arsu/vault`)                                |
| Its previous versions | `vault.bak.1` to `vault.bak.3`, next to it                                               |
| Settings              | `$XDG_CONFIG_HOME/arsu/settings.json` (`~/.config/arsu/settings.json`)                   |
| This device's id      | `$XDG_STATE_HOME/arsu/installation-id` (`~/.local/state/arsu/installation-id`), for sync |
| Diagnostic log        | `~/.local/share/io.github.amad3v.arsu/logs/`                                             |

On Android, they live in the app's private storage, `/data/user/0/io.github.amad3v.arsu/` (`vault/vault`, `settings/settings.json` and `sync/installation-id`), which no other app can read. The vault is left out of Android's cloud backup, and the device's id stays behind when you move to a new phone, so the new phone is a device of its own. To keep a copy, use **Export**: it writes an encrypted Aegis backup wherever you choose.

**Back up the vault file.** It is the whole backup: copy it anywhere, since it can't be read without your master password.

**Don't forget your master password.** It can't be recovered or reset, and neither can the accounts without it. Keep your services' recovery codes somewhere safe too.

## Security

What Arsu protects against:

- **A stolen computer, disk or backup of your files.** The vault resists offline password guessing for as long as your master password's strength allows. Choose a long one.
- **The interface itself.** The secrets never leave the Rust side of the app: the interface only ever receives codes. It loads nothing from the network (a strict Content Security Policy), and can call only the app's own commands.
- **Leftovers.** Copied codes are cleared from the clipboard, and the app writes no core dumps that could hold the unlocked vault.

What it can't protect against:

- **Malware running as your user while the vault is unlocked.** Such a program can read your screen, your clipboard and your keystrokes. Lock the vault when you're not using it; it also locks itself when you're idle.
- **Screen capture.** Linux offers no reliable way for an app to block it. (Android does: there, Arsu's window can't be captured in screenshots, recordings or the recent apps list.)

### Diagnostic report

When something goes wrong, a diagnostic report tells why, on any device. Arsu logs warnings, errors and crashes as [JSON Lines](https://jsonlines.org), each with its time (UTC), where it happened, the error's kind and causes, and the steps that led to it; routine activity is not written. **About → Save diagnostic report** shows you everything the report holds, then saves it as one zip file (the log, and what the app and the device are: versions, operating system, phone model) wherever you choose, to attach to an issue. It never holds an account, a secret, a code, an invitation, a password or a device's name; home directories and Bluetooth addresses are masked as each line is written, and it never leaves your device on its own. The log lives in the app's log directory (`~/.local/share/io.github.amad3v.arsu/logs/` on Linux, the app's private storage on Android), five files of 1 MB at most.

### Reporting a vulnerability

Please report security issues privately, through GitHub's [private vulnerability reporting](https://github.com/amad3v/arsu/security/advisories/new), not in a public issue.

## Building from source

Requirements:

- Rust, at the version pinned in `rust-toolchain.toml`: run `rustup toolchain install` in the project folder
- Node.js and pnpm
- The [Tauri prerequisites for Linux](https://v2.tauri.app/start/prerequisites/#linux), notably WebKitGTK 4.1
- The D-Bus headers, for Bluetooth sync through BlueZ (`libdbus-1-dev` on Debian and Ubuntu, `dbus-devel` on Fedora)

```sh
pnpm install
pnpm tauri dev     # run with hot reload
pnpm tauri build   # .deb and .rpm packages under target/release/bundle/
```

### Building for Android

Additionally:

- The [Tauri prerequisites for Android](https://v2.tauri.app/start/prerequisites/#android): Android Studio (or the SDK command-line tools), the NDK, Java 17 or later, and `ANDROID_HOME` and `NDK_HOME` set
- The Rust Android targets: `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`

```sh
pnpm tauri android dev     # run on a connected device or emulator, with hot reload
pnpm tauri android build   # APKs and AABs under src-tauri/gen/android/app/build/outputs/
```

Release builds have to be signed: see [Tauri's guide to Android signing](https://v2.tauri.app/distribute/sign/android/).

The Android project is in `src-tauri/gen/android`. Its manifest asks for no network permission outside debug builds (which load the interface from the development server); for Bluetooth sync it asks for Nearby devices (Android 12 and later: to find other devices and to wait for them) or Bluetooth and location (Android 8 to 11, where scanning needs it), and for the camera to scan QR codes, each only when first needed; CI fails a release build that asks for anything else. `MainActivity.kt` blocks screen capture and keeps the interface clear of the system bars.

Checks, as CI runs them:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm lint && pnpm typecheck && pnpm fmt:check && pnpm test
```

### Layout

| Path         | What                                                                                                                                                                                                                                                                  |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/`    | The Rust core: `crypto`, `vault-core`, `otp`, `qr`, `storage`, `interop`, `vault-sync` (merging, devices, encrypted sessions and sync files), `ble-linux` and `ble-android` (Bluetooth LE through BlueZ and on Android), and `cmd` (the commands the interface calls) |
| `src-tauri/` | The app shell: process hardening, window, command permissions, Content Security Policy; the Android project in `gen/android/`                                                                                                                                         |
| `src/`       | The interface, in SolidJS                                                                                                                                                                                                                                             |

## License

[MIT](LICENSE) © 2026 Mohamed Jouini
