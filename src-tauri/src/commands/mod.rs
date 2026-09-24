pub mod consent;
pub mod debug;
pub mod device;
pub mod integration;
pub mod migration;
pub mod partner_secret;
pub mod rewards;
pub mod settings;
pub mod system;
pub mod updates;

#[cfg(test)]
mod force_reinstall_rearm_tests;

#[cfg(test)]
mod integration_update_lock_tests;
