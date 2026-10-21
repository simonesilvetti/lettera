//! Lettera core: everything that is not UI. No Tauri types in here.

pub mod autostart;
pub mod config;
pub mod db;
pub mod engine;
pub mod error;
pub mod events;
pub mod imap;
pub mod model;
pub mod oauth;
pub mod parse;
pub mod privacy;
pub mod reading;
pub mod secrets;
pub mod smtp;
pub mod sync;
pub mod theme;
pub mod thread;

pub use engine::Engine;
pub use error::{Error, Result};
