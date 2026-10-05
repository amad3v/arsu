### Install

Check the downloads, then install the package for your system:

```sh
curl -sL https://github.com/amad3v.gpg | gpg --import   # the signing key, once
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS
```

- Debian, Ubuntu: `sudo apt install ./arsu_${VERSION}_amd64.deb`
- Fedora, openSUSE: `sudo dnf install ./arsu-${VERSION}-1.x86_64.rpm`
- Arch Linux: `arsu` (built from source) or `arsu-bin` (this release's binary) on the AUR
- Android 7 or later: `arsu-${VERSION}.apk`. Its signing certificate's SHA-256 is
  `01:0E:E6:98:8E:6E:92:29:84:01:67:D4:19:8F:4B:B2:E9:BC:5F:93:29:AA:11:FC:61:C0:6B:CB:32:E7:32:9B`
  (`apksigner verify --print-certs`, or an app such as AppVerifier, shows it).

`arsu` is the bare binary, for packagers; it needs WebKitGTK 4.1.

Signing key: Mohamed Jouini <amad3v@gmail.com>, fingerprint
`6A70 0E00 3968 20D9 3A82  9FF9 1CAC 141C 3451 6CB6`.
