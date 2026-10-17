//! Offline vault sync, without I/O.
//!
//! Everything sync decides lives here, as pure functions and state
//! machines that take bytes in and give bytes out: how two vaults merge,
//! which devices are trusted, what goes over the wire and how it is
//! encrypted. Moving those bytes — over Bluetooth or in a file — is the
//! callers' job, so every rule here is tested without a radio.

pub mod devices;
pub mod file;
pub mod gatt;
pub mod identity;
pub mod invitation;
mod merge;
pub mod noise;
pub mod protocol;
pub mod session;
pub mod wire;

pub use merge::{MergeSummary, Replica, merge};
