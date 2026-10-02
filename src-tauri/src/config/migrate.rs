//! B5: config schema migration, as one named pair of pure functions.
//!
//! FEM has no version stamp and needs none: the schema has been purely
//! ADDITIVE from v0.4.14 to v0.4.33 (`storage_dir` appears at v0.4.30,
//! `pending_install_id` at v0.4.31, `debug_logging_enabled` at v0.4.32, and
//! `#[serde(flatten)] extra` has existed since v0.4.14), and every field added
//! after v0.4.14 carries a serde default. So "migrate a captured config to the
//! current schema with defaults filled and user values preserved" IS
//! `parse` followed by `to_disk_string`:
//!
//!   * defaults filled — serde fills every absent field from its `default`;
//!   * user values preserved — every recognised field round-trips, and every
//!     key this build does NOT recognise survives in `FemConfig::extra`
//!     (`#[serde(flatten)]`), so a config written by a NEWER FEM is not
//!     stripped by an older one;
//!   * idempotent — `integrations_enabled` and `integration_versions`
//!     serialise in key order (`ordered_map`), so running it twice produces
//!     byte-identical output.
//!
//! These are the same two calls `ConfigStore` makes on every load and every
//! save, so what the tests exercise is what the app actually runs, rather than
//! a parallel implementation that could drift from it.

use serde_json::Error;

use super::FemConfig;

/// Read one raw `fem_config.json` document, filling defaults for anything it
/// does not carry.
pub fn parse(raw: &str) -> Result<FemConfig, Error> {
    serde_json::from_str::<FemConfig>(raw)
}

/// Render a config to the exact bytes FEM writes to disk.
pub fn to_disk_string(config: &FemConfig) -> Result<String, Error> {
    serde_json::to_string_pretty(config)
}
