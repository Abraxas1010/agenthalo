//! Cockpit module — interactive agent terminal management.
//!
//! Provides PTY-backed terminal sessions accessible via WebSocket,
//! with support for up to 10 concurrent sessions.

pub mod deploy;
pub mod pty_manager;
pub mod session;
pub mod ws_bridge;
