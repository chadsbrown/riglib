//! Shared IO task for text-protocol rig backends (Kenwood, Elecraft, Yaesu).
//!
//! This crate provides the single-IO-task pattern for semicolon-terminated
//! text protocols. One tokio task owns the transport exclusively and handles
//! command/response exchanges, SET command drain, unsolicited AI (Auto
//! Information) frame processing, and graceful shutdown.
//!
//! # Architecture
//!
//! - [`protocol`] — shared decode/encode for `;`-terminated responses
//! - [`io`] — IO task types, spawn, and the select loop

pub mod io;
pub mod protocol;
