/// Calls `$callback!` with the name of every command the frontend may
/// invoke, all of them in `cmd::commands`.
///
/// This is the one list of app commands: `build.rs` reads it to generate
/// an `allow-<command>` permission for each (which makes Tauri deny any
/// command not granted in a capability), and `lib.rs` reads it to
/// register the handlers, so the two cannot drift apart. A new command
/// also has to be granted in `permissions/app-commands.toml`, which a
/// test in `lib.rs` checks.
macro_rules! with_app_commands {
  ($callback:ident) => {
    $callback! {
      vault_exists,
      create_vault,
      unlock_vault,
      lock_vault,
      is_unlocked,
      record_activity,
      list_entries,
      get_current_code,
      copy_code,
      add_entry_from_uri,
      add_entry_manual,
      delete_entry,
      export_entry_qr,
      pick_import_file,
      import_file,
      export_to_aegis_file,
      pick_qr_image,
      get_settings,
      update_settings,
      get_about_info,
      copy_about_details,
      open_link,
      show_window,
    }
  };
}
