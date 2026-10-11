//! HTTP route handlers, one module per API area (HS2-DPPCNN). The router itself is
//! built in `crate::app`.

pub(crate) mod accounts;
pub(crate) mod activity;
pub(crate) mod auth;
pub(crate) mod checkout_tickets;
pub(crate) mod checkouts;
pub(crate) mod claims;
pub(crate) mod permissions;
pub(crate) mod project_tools;
pub(crate) mod providers;
pub(crate) mod store_scoped;
pub(crate) mod terminals;
pub(crate) mod ticket_writes;
pub(crate) mod tickets;
pub(crate) mod transfer;
