use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Local;

use crate::api::types::ApiPocSlot;

/// On-disk JSONL cache for submitted PoC slots.
///
/// One file per calendar day under `<app_data_dir>/poc_slots/<YYYY-MM-DD>.jsonl`.
/// Each 10-minute tick appends one line. Reads deduplicate by slot number so the
/// latest value for a slot wins and callers always get a stable 0..143 sequence.
pub struct PocCache {
    root: PathBuf,
}

impl PocCache {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf().join("poc_slots"),
        }
    }

    /// Append a submitted slot to today's JSONL file.
    pub fn append(&self, slot: &ApiPocSlot) -> Result<()> {
        let date = Local::now().format("%Y-%m-%d").to_string();
        let path = self.root.join(format!("{}.jsonl", date));
        fs::create_dir_all(&self.root)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        let line = serde_json::to_string(slot)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// Load all unique slots for a given date (`YYYY-MM-DD`).
    pub fn load(&self, date: &str) -> Result<Vec<ApiPocSlot>> {
        let path = self.root.join(format!("{}.jsonl", date));
        if !path.exists() {
            return Ok(vec![]);
        }

        let file = fs::File::open(&path)?;
        let reader = BufReader::new(file);
        let mut by_slot: HashMap<u32, ApiPocSlot> = HashMap::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let slot: ApiPocSlot = serde_json::from_str(&line)?;
            by_slot.insert(slot.slot_number, slot);
        }

        let mut slots: Vec<ApiPocSlot> = by_slot.into_values().collect();
        slots.sort_by_key(|s| s.slot_number);
        Ok(slots)
    }
}
