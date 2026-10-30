//! A checkout's saved views (`views` in its shared settings). The storage, validation, and
//! query evaluation live in `hotsheet_ticketing::saved_views` so notification rules can test a
//! ticket against a view outside the server (HS2-G8NXNC).

pub use hotsheet_ticketing::saved_views::{
    SavedView as CustomView, from_settings, replace, validate,
};
