//! The few Android framework calls the app makes from Rust, through JNI:
//! the clipboard, opening a link, reading or writing a document the user
//! picked in the system's file picker, and whether the phone is locked.
//!
//! On Linux these go through GTK and `arboard`; on Android they are
//! Java APIs, and calling them from here keeps the same split as on the
//! desktop: the `WebView` is granted no plugin command at all, and only
//! ever sees what the commands in [`crate::commands`] return.
//!
//! Every call runs on the calling thread, attached to the Java VM for its
//! duration, in its own JNI local frame. None of these APIs needs the UI
//! thread. The context used is the application's, which Tauri's Android
//! glue (`tao`) publishes through `ndk_context` at start-up.

use std::{
  fmt,
  fs::File,
  os::fd::{FromRawFd, OwnedFd},
};

use jni::{
  JNIEnv, JavaVM,
  objects::{JObject, JString, JValue},
};

/// A Java call that failed: the exception it threw, or the JNI failure.
#[derive(Debug)]
pub struct JavaError(String);

impl fmt::Display for JavaError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for JavaError {}

/// `Intent.FLAG_ACTIVITY_NEW_TASK`: required to start an activity from
/// the application context, which is not an activity itself.
const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;
/// `ClipDescription.EXTRA_IS_SENSITIVE`, by value: the constant only
/// exists from API 33, the key is honoured from API 24 (where clip
/// extras appeared) by clipboard previews and keyboards that read it.
const EXTRA_IS_SENSITIVE: &str = "android.content.extra.IS_SENSITIVE";
/// `OpenableColumns.DISPLAY_NAME`.
const DISPLAY_NAME: &str = "_display_name";
/// The API levels of the calls that are not available on every version
/// the app supports.
const API_CLIP_EXTRAS: i32 = 24;
const API_CLEAR_PRIMARY_CLIP: i32 = 28;

type JniResult<T> = jni::errors::Result<T>;

/// Runs `call` with a JNI environment attached to this thread and the
/// application context, turning a thrown exception into a [`JavaError`]
/// (and clearing it, so the thread can keep using JNI).
fn with_context<T>(
  call: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> JniResult<T>,
) -> Result<T, JavaError> {
  let android = ndk_context::android_context();
  // SAFETY: `ndk_context` holds the process's Java VM and a global
  // reference to the application context, both valid for the life of
  // the process once Tauri's Android glue has started.
  let vm = unsafe { JavaVM::from_raw(android.vm().cast()) }.map_err(|error| jni_error(&error))?;
  let mut env = vm
    .attach_current_thread()
    .map_err(|error| jni_error(&error))?;
  // SAFETY: as above; the reference is global, so it is valid on any
  // thread, and wrapping it in a `JObject` never deletes it.
  let context = unsafe { JObject::from_raw(android.context().cast()) };
  env
    .with_local_frame(16, |env| call(env, &context))
    .map_err(|error| match take_exception(&mut env) {
      Some(exception) => JavaError(exception),
      None => jni_error(&error),
    })
}

fn jni_error(error: &jni::errors::Error) -> JavaError {
  JavaError(format!("JNI call failed: {error}"))
}

/// Clears the pending Java exception, if there is one, and describes it.
fn take_exception(env: &mut JNIEnv<'_>) -> Option<String> {
  if !env.exception_check().unwrap_or(false) {
    return None;
  }
  let throwable = env.exception_occurred().ok();
  env.exception_clear().ok()?;
  let description = env
    .call_method(throwable?, "toString", "()Ljava/lang/String;", &[])
    .and_then(jni::objects::JValueGen::l)
    .ok()?;
  java_string(env, description).ok()
}

fn java_string(env: &mut JNIEnv<'_>, object: JObject<'_>) -> JniResult<String> {
  Ok(env.get_string(&JString::from(object))?.into())
}

fn sdk_int(env: &mut JNIEnv<'_>) -> JniResult<i32> {
  env
    .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
    .i()
}

fn parse_uri<'local>(env: &mut JNIEnv<'local>, uri: &str) -> JniResult<JObject<'local>> {
  let uri = env.new_string(uri)?;
  env
    .call_static_method(
      "android/net/Uri",
      "parse",
      "(Ljava/lang/String;)Landroid/net/Uri;",
      &[JValue::from(&uri)],
    )?
    .l()
}

/// The system service named by the `Context` constant `name`, e.g.
/// `CLIPBOARD_SERVICE`.
fn system_service<'local>(
  env: &mut JNIEnv<'local>,
  context: &JObject<'_>,
  name: &str,
) -> JniResult<JObject<'local>> {
  let service = env
    .get_static_field("android/content/Context", name, "Ljava/lang/String;")?
    .l()?;
  env
    .call_method(
      context,
      "getSystemService",
      "(Ljava/lang/String;)Ljava/lang/Object;",
      &[JValue::from(&service)],
    )?
    .l()
}

// ---- the lock screen -----------------------------------------------------

/// Whether the phone is locked: its screen is off, or the lock screen
/// shows. Either way, no one should come back to an unlocked vault.
///
/// # Errors
///
/// Returns the Java exception if a call fails.
pub fn phone_locked() -> Result<bool, JavaError> {
  with_context(|env, context| {
    let power = system_service(env, context, "POWER_SERVICE")?;
    if !env.call_method(&power, "isInteractive", "()Z", &[])?.z()? {
      return Ok(true);
    }
    let keyguard = system_service(env, context, "KEYGUARD_SERVICE")?;
    env
      .call_method(&keyguard, "isKeyguardLocked", "()Z", &[])?
      .z()
  })
}

// ---- the clipboard -------------------------------------------------------

fn clipboard_manager<'local>(
  env: &mut JNIEnv<'local>,
  context: &JObject<'_>,
) -> JniResult<JObject<'local>> {
  system_service(env, context, "CLIPBOARD_SERVICE")
}

fn plain_text_clip<'local>(env: &mut JNIEnv<'local>, text: &str) -> JniResult<JObject<'local>> {
  let label = env.new_string("")?;
  let text = env.new_string(text)?;
  env
    .call_static_method(
      "android/content/ClipData",
      "newPlainText",
      "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;",
      &[JValue::from(&label), JValue::from(&text)],
    )?
    .l()
}

/// Places `text` on the clipboard, marked as sensitive: Android 13's
/// clipboard preview then shows dots instead of it, and keyboards that
/// suggest recent clips leave it out.
///
/// # Errors
///
/// Returns the exception the clipboard service threw.
pub fn set_clipboard_text(text: &str) -> Result<(), JavaError> {
  with_context(|env, context| {
    let manager = clipboard_manager(env, context)?;
    let clip = plain_text_clip(env, text)?;
    if sdk_int(env)? >= API_CLIP_EXTRAS {
      let extras = env.new_object("android/os/PersistableBundle", "()V", &[])?;
      let key = env.new_string(EXTRA_IS_SENSITIVE)?;
      env.call_method(
        &extras,
        "putBoolean",
        "(Ljava/lang/String;Z)V",
        &[JValue::from(&key), JValue::Bool(1)],
      )?;
      let description = env
        .call_method(
          &clip,
          "getDescription",
          "()Landroid/content/ClipDescription;",
          &[],
        )?
        .l()?;
      env.call_method(
        &description,
        "setExtras",
        "(Landroid/os/PersistableBundle;)V",
        &[JValue::from(&extras)],
      )?;
    }
    env.call_method(
      &manager,
      "setPrimaryClip",
      "(Landroid/content/ClipData;)V",
      &[JValue::from(&clip)],
    )?;
    Ok(())
  })
}

/// The clipboard's text, or `None` if it holds none — or if the app may
/// not read it: from Android 10, only the app in the foreground can.
///
/// # Errors
///
/// Returns the exception the clipboard service threw.
pub fn clipboard_text() -> Result<Option<String>, JavaError> {
  with_context(|env, context| {
    let manager = clipboard_manager(env, context)?;
    let clip = env
      .call_method(
        &manager,
        "getPrimaryClip",
        "()Landroid/content/ClipData;",
        &[],
      )?
      .l()?;
    if clip.is_null() || env.call_method(&clip, "getItemCount", "()I", &[])?.i()? < 1 {
      return Ok(None);
    }
    let item = env
      .call_method(
        &clip,
        "getItemAt",
        "(I)Landroid/content/ClipData$Item;",
        &[JValue::Int(0)],
      )?
      .l()?;
    let text = env
      .call_method(&item, "getText", "()Ljava/lang/CharSequence;", &[])?
      .l()?;
    if text.is_null() {
      return Ok(None);
    }
    let text = env
      .call_method(&text, "toString", "()Ljava/lang/String;", &[])?
      .l()?;
    java_string(env, text).map(Some)
  })
}

/// Empties the clipboard: `clearPrimaryClip` where it exists (API 28),
/// otherwise by replacing the clip with an empty one.
///
/// # Errors
///
/// Returns the exception the clipboard service threw.
pub fn clear_clipboard() -> Result<(), JavaError> {
  with_context(|env, context| {
    let manager = clipboard_manager(env, context)?;
    if sdk_int(env)? >= API_CLEAR_PRIMARY_CLIP {
      env.call_method(&manager, "clearPrimaryClip", "()V", &[])?;
    } else {
      let empty = plain_text_clip(env, "")?;
      env.call_method(
        &manager,
        "setPrimaryClip",
        "(Landroid/content/ClipData;)V",
        &[JValue::from(&empty)],
      )?;
    }
    Ok(())
  })
}

// ---- links ---------------------------------------------------------------

/// Opens `url` in the app the user chose for it, usually their browser.
///
/// # Errors
///
/// Returns the exception `startActivity` threw, typically
/// `ActivityNotFoundException` when no installed app opens links.
pub fn open_url(url: &str) -> Result<(), JavaError> {
  with_context(|env, context| {
    let action = env
      .get_static_field(
        "android/content/Intent",
        "ACTION_VIEW",
        "Ljava/lang/String;",
      )?
      .l()?;
    let uri = parse_uri(env, url)?;
    let intent = env.new_object(
      "android/content/Intent",
      "(Ljava/lang/String;Landroid/net/Uri;)V",
      &[JValue::from(&action), JValue::from(&uri)],
    )?;
    env.call_method(
      &intent,
      "addFlags",
      "(I)Landroid/content/Intent;",
      &[JValue::Int(FLAG_ACTIVITY_NEW_TASK)],
    )?;
    env.call_method(
      context,
      "startActivity",
      "(Landroid/content/Intent;)V",
      &[JValue::from(&intent)],
    )?;
    Ok(())
  })
}

// ---- documents -----------------------------------------------------------

fn content_resolver<'local>(
  env: &mut JNIEnv<'local>,
  context: &JObject<'_>,
) -> JniResult<JObject<'local>> {
  env
    .call_method(
      context,
      "getContentResolver",
      "()Landroid/content/ContentResolver;",
      &[],
    )?
    .l()
}

/// How to open a document: `"r"` to read it, `"wt"` to replace its
/// contents (write, truncating).
#[derive(Debug, Clone, Copy)]
pub enum DocumentMode {
  Read,
  Replace,
}

impl DocumentMode {
  const fn as_str(self) -> &'static str {
    match self {
      Self::Read => "r",
      Self::Replace => "wt",
    }
  }
}

/// Opens the document at `uri` (a `content://` URI the file picker
/// returned) as a file descriptor the process owns.
///
/// # Errors
///
/// Returns the exception the content provider threw, typically
/// `FileNotFoundException` or `SecurityException`.
pub fn open_document(uri: &str, mode: DocumentMode) -> Result<File, JavaError> {
  let fd = with_context(|env, context| {
    let resolver = content_resolver(env, context)?;
    let uri = parse_uri(env, uri)?;
    let mode = env.new_string(mode.as_str())?;
    let descriptor = env
      .call_method(
        &resolver,
        "openFileDescriptor",
        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/os/ParcelFileDescriptor;",
        &[JValue::from(&uri), JValue::from(&mode)],
      )?
      .l()?;
    if descriptor.is_null() {
      return Ok(None);
    }
    // Hands the descriptor over: Java no longer closes it.
    env
      .call_method(&descriptor, "detachFd", "()I", &[])?
      .i()
      .map(Some)
  })?
  .ok_or_else(|| JavaError("the document provider returned no file".to_owned()))?;
  // SAFETY: `detachFd` gave up the descriptor, which is open, to us.
  Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

/// The name the document at `uri` is shown under, such as
/// `aegis-export.json`; `None` if its provider doesn't say.
///
/// # Errors
///
/// Returns the exception the content provider threw.
pub fn document_name(uri: &str) -> Result<Option<String>, JavaError> {
  with_context(|env, context| {
    let resolver = content_resolver(env, context)?;
    let uri = parse_uri(env, uri)?;
    let column = env.new_string(DISPLAY_NAME)?;
    let projection = env.new_object_array(1, "java/lang/String", &column)?;
    let none = JObject::null();
    let cursor = env
      .call_method(
        &resolver,
        "query",
        "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;",
        &[
          JValue::from(&uri),
          JValue::from(&projection),
          JValue::from(&none),
          JValue::from(&none),
          JValue::from(&none),
        ],
      )?
      .l()?;
    if cursor.is_null() {
      return Ok(None);
    }
    let name = read_first_string(env, &cursor);
    env.call_method(&cursor, "close", "()V", &[])?;
    name
  })
}

fn read_first_string(env: &mut JNIEnv<'_>, cursor: &JObject<'_>) -> JniResult<Option<String>> {
  if !env.call_method(cursor, "moveToFirst", "()Z", &[])?.z()? {
    return Ok(None);
  }
  let value = env
    .call_method(
      cursor,
      "getString",
      "(I)Ljava/lang/String;",
      &[JValue::Int(0)],
    )?
    .l()?;
  if value.is_null() {
    return Ok(None);
  }
  java_string(env, value).map(Some)
}
