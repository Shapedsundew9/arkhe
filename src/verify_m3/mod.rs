//! Milestone 3 (`H4`) free-rider resistance and verified local excision verification module.
//!
//! Phase 1 provides the [`detours`] synthetic free-rider library (`D0`–`D7b`) across widths
//! $k \in \{1, 2\}$ and [`crate::substrate::WireDelayMode`] (`Combinational` & `TicksPerHop`).

pub mod detours;

pub use detours::{DetourClass, DetourEdit, build_detour_edit};
