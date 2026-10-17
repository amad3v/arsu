//! Arsu as a Bluetooth LE peripheral on Linux, through `BlueZ`: it
//! advertises Arsu's service and serves its two characteristics,
//! and moves chunks between them and the app. It decides nothing: the
//! chunks are those of a `vault_sync::gatt::GattLink`, which the caller
//! runs.
//!
//! * [`GattServer`] is Arsu's service, registered once for the app's
//!   life so that its handles stay put; each session advertises with an
//!   [`Advertised`] and ends when its central disconnects.
//! * The central's writes to `rx` arrive through
//!   [`GattServer::next_chunk`]; [`GattServer::send`] notifies on `tx`.
//! * One central per session: the first to write is served, and writes
//!   from any other device are refused until the next session.
//!
//! [`scan`] and [`Central`] are the other side, for computer-to-computer
//! sync: find Arsu peripherals, connect, subscribe, write.
//!
//! Advertising stops when the session's [`Advertised`] is dropped: the
//! sync screen owns it.

#![cfg(target_os = "linux")]

mod central;
mod peripheral;

pub use bluer::Address;
pub use central::{Central, Found, scan};
pub use peripheral::{Advertised, BleError, CentralGate, Event, GattServer, has_left};
