//! A touchpad's pinch, kept from zooming the app on Linux.
//!
//! `WebKitGTK` zooms the page on a pinch: its view has a `GtkGestureZoom`,
//! which GTK 3 feeds touchpad pinches as well as touchscreen ones, and no
//! setting turns it off (wry's zoom switch does nothing on Linux; the
//! keyboard and Ctrl+scroll zoom nothing there to begin with). A
//! touchscreen's pinch reaches the page, which takes it from the engine
//! (the frontend's `suppressPinchZoom`); a touchpad's never does. So the
//! view gets a zoom gesture of its own, run in the capture phase, before
//! the engine's: it claims touchpad pinches, which then go no further, and
//! lets touchscreen ones through to the page.

use std::cell::RefCell;

use gtk::{EventSequenceState, GestureZoom, PropagationPhase, gdk::EventType, prelude::*};
use tauri::WebviewWindow;

thread_local! {
  /// The gestures, for as long as the app runs: in GTK 3 a widget does
  /// not keep its gestures alive. Only GTK's main thread touches them.
  static GESTURES: RefCell<Vec<GestureZoom>> = const { RefCell::new(Vec::new()) };
}

/// Keeps a touchpad's pinch from zooming `window`'s page.
///
/// # Errors
///
/// If Tauri cannot reach the window's `WebView`.
pub fn forbid_touchpad_zoom(window: &WebviewWindow) -> tauri::Result<()> {
  window.with_webview(|webview| {
    let gesture = GestureZoom::new(&webview.inner());
    gesture.set_propagation_phase(PropagationPhase::Capture);
    gesture.connect_begin(|gesture, sequence| {
      let touchpad = gesture
        .last_event(sequence)
        .is_some_and(|event| event.event_type() == EventType::TouchpadPinch);
      gesture.set_state(if touchpad {
        EventSequenceState::Claimed
      } else {
        EventSequenceState::Denied
      });
    });
    GESTURES.with_borrow_mut(|gestures| gestures.push(gesture));
  })
}
