# Arsu

**Offline TOTP/HOTP authenticator with an encrypted local vault**, for the Linux desktop.

Arsu keeps your two-factor secrets in a single encrypted file on your machine and shows their codes. It never connects to the network: no account, no sync, no telemetry.

<!-- Screenshot: add .github/screenshot.png and uncomment.
![Arsu's main window](.github/screenshot.png)
-->

Named after Arsu, the god of the evening star worshipped at Palmyra, who protected the caravans crossing the desert.

## Features

- **Encrypted vault.** Your master password is stretched with Argon2id (256 MiB, 3 passes), and the whole vault is encrypted with XChaCha20-Poly1305. The file is useless without the password.
- **Crash-safe saves** that never leave a half-written vault, plus the last three versions kept next to it, in case of a bad edit.
- **Add accounts** from an `otpauth://` link, a screenshot of a QR code (paste it, drop it, or pick the file), or by hand.
- **TOTP and HOTP**, SHA-1/256/512, 6–8 digits, with the next code shown before the current one expires.
- **Quick to use from the keyboard:** type to search, arrows to choose, Enter to copy. Or double-click a row.
- **Locks itself** after an idle time you choose (5 minutes by default, counting time the computer sleeps).
- **Clears copied codes** from the clipboard after a delay you choose (20 seconds by default), unless you copied something else meanwhile, and asks clipboard managers not to keep them in their history.
- **Imports** Aegis and 2FAS backups, plain or encrypted, skipping accounts you already have. **Exports** an encrypted Aegis backup, or a single account as a QR code (after asking for your master password again).
- **Light and dark themes**, or following the system.

## Install

### From a release

Download the package for your distribution from the [Releases page](https://github.com/amad3v/arsu/releases), check it against `SHA256SUMS`, and install it:

```sh
sha256sum --check --ignore-missing SHA256SUMS

# Debian, Ubuntu and derivatives
sudo apt install ./arsu_1.0.0_amd64.deb

# Fedora, openSUSE and other RPM-based distributions
sudo dnf install ./arsu-1.0.0-1.x86_64.rpm
```

### Arch Linux

Arsu is on the AUR as [`arsu`](https://aur.archlinux.org/packages/arsu), built from source:

```sh
yay -S arsu   # or any AUR helper, or git clone + makepkg
```

## Your data

| What                  | Where                                                                  |
| --------------------- | ---------------------------------------------------------------------- |
| Vault                 | `$XDG_DATA_HOME/arsu/vault` (`~/.local/share/arsu/vault`)              |
| Its previous versions | `vault.bak.1` to `vault.bak.3`, next to it                             |
| Settings              | `$XDG_CONFIG_HOME/arsu/settings.json` (`~/.config/arsu/settings.json`) |

**Back up the vault file.** It is the whole backup: copy it anywhere, since it can't be read without your master password.

**Don't forget your master password.** It can't be recovered or reset, and neither can the accounts without it. Keep your services' recovery codes somewhere safe too.

## Security

What Arsu protects against:

- **A stolen laptop, disk or backup of your files.** The vault resists offline password guessing for as long as your master password's strength allows. Choose a long one.
- **The interface itself.** The secrets never leave the Rust side of the app: the interface only ever receives codes. It loads nothing from the network (a strict Content Security Policy), and can call only the app's own commands.
- **Leftovers.** Copied codes are cleared from the clipboard, and the app writes no core dumps that could hold the unlocked vault.

What it can't protect against:

- **Malware running as your user while the vault is unlocked.** Such a program can read your screen, your clipboard and your keystrokes. Lock the vault when you're not using it; it also locks itself when you're idle.
- **Screen capture.** Linux offers no reliable way for an app to block it.

### Reporting a vulnerability

Please report security issues privately, through GitHub's [private vulnerability reporting](https://github.com/amad3v/arsu/security/advisories/new), not in a public issue.

## Building from source

Requirements:

- Rust, at the version pinned in `rust-toolchain.toml`: run `rustup toolchain install` in the project folder
- Node.js and pnpm
- The [Tauri prerequisites for Linux](https://v2.tauri.app/start/prerequisites/#linux), notably WebKitGTK 4.1

```sh
pnpm install
pnpm tauri dev     # run with hot reload
pnpm tauri build   # .deb and .rpm packages under target/release/bundle/
```

Checks, as CI runs them:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm lint && pnpm typecheck && pnpm fmt:check && pnpm test
```

### Layout

| Path         | What                                                                                                                   |
| ------------ | ---------------------------------------------------------------------------------------------------------------------- |
| `crates/`    | The Rust core: `crypto`, `vault-core`, `otp`, `qr`, `storage`, `interop`, and `cmd` (the commands the interface calls) |
| `src-tauri/` | The app shell: process hardening, window, command permissions, Content Security Policy                                 |
| `src/`       | The interface, in SolidJS                                                                                              |

## License

[MIT](LICENSE) © 2026 Mohamed Jouini
