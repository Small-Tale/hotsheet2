//! First-party notifiers. Each one is a descriptor plus a delivery implementation over an
//! injectable transport.

pub mod email;
pub mod ntfy;
pub mod slack;
pub mod speak;
pub mod teams;
pub mod twilio;
pub mod webhook;
