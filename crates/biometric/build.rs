//! Hands the Android library in `android/` to the app's Gradle build (the
//! `links` key names it). The plugin has no commands: the `WebView` never
//! calls it, only Rust does.

fn main() {
  tauri_plugin::Builder::new(&[])
    .android_path("android")
    .build();
}
