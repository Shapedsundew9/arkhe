//! Milestone 3 (`H4`) free-rider resistance and verified local excision verification module.
//!
//! Phase 1 provides the [`detours`] synthetic free-rider library (`D0`–`D7b`) across widths
//! $k \in \{1, 2\}$ and [`crate::substrate::WireDelayMode`] (`Combinational` & `TicksPerHop`).
//! Phase 2 provides [`explore`] (`arkhe explore-m3`) for exploratory and paired-ablation sweeps.

pub mod detours;
pub mod explore;

pub use detours::{DetourClass, DetourEdit, build_detour_edit};
pub use explore::{ExploreM3Report, check_explore_m3_seed_range_holdout, run_explore_m3};
