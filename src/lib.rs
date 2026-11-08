//! Found: a desktop app for scans of found objects — crop, develop, glitch, tag, file, collage.
#![allow(clippy::needless_range_loop)]

pub mod app;
pub mod collage;
pub mod graph;
pub mod img;
pub mod io;
pub mod keys;
pub mod library;
pub mod ops;
pub mod pdf;
pub mod presets;
pub mod settings;
pub mod ui;
pub mod undo;
pub mod worker;

pub use app::{AppConfig, FoundApp};
