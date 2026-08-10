use std::collections::VecDeque;

use crate::api::types::ApiPocHardwareDoc;

/// One failed slot submission retained for later retry.
#[derive(Debug, Clone)]
pub struct QueuedSlot {
    /// UTC date ("YYYY-MM-DD") the slot belongs to. Slots are per-day server
    /// side, so an entry from a previous day must never be resubmitted — it
    /// would land on the wrong day's record.
    pub utc_date: String,
    pub slot_number: u32,
    pub doc: ApiPocHardwareDoc,
}

/// Bounded in-memory retry queue for failed PoC slot submissions (v0.4.7
/// field reports: a connection outage silently dropped every slot in the
/// window — 0/144 slots despite healthy integrations). One entry per
/// (utc_date, slot_number); newest doc wins. In-memory only: an app restart
/// loses the backlog (accepted — the queue exists to survive network blips,
/// not restarts).
#[derive(Debug, Default)]
pub struct PocRetryQueue {
    entries: VecDeque<QueuedSlot>,
}

/// One slot per 10 minutes — a full day is the most a backlog can usefully hold.
pub const RETRY_QUEUE_CAP: usize = 144;

impl PocRetryQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Queue a failed slot submission. Replaces any existing entry for the
    /// same (utc_date, slot_number) — the freshest health snapshot for a slot
    /// is the truthful one. Evicts the oldest entry beyond the cap.
    pub fn push(&mut self, utc_date: &str, slot_number: u32, doc: ApiPocHardwareDoc) {
        self.entries
            .retain(|e| !(e.utc_date == utc_date && e.slot_number == slot_number));
        self.entries.push_back(QueuedSlot {
            utc_date: utc_date.to_string(),
            slot_number,
            doc,
        });
        while self.entries.len() > RETRY_QUEUE_CAP {
            self.entries.pop_front();
        }
    }

    /// Remove and return up to `max` entries for `today` (oldest first).
    /// Entries from other days are silently discarded — they can no longer be
    /// submitted truthfully.
    pub fn take_batch(&mut self, today: &str, max: usize) -> Vec<QueuedSlot> {
        self.entries.retain(|e| e.utc_date == today);
        let n = max.min(self.entries.len());
        self.entries.drain(..n).collect()
    }

    /// Return a drained entry that failed to submit so the next tick retries it.
    pub fn requeue(&mut self, entry: QueuedSlot) {
        // Front, not back: it is still the oldest pending slot.
        self.entries.push_front(entry);
        while self.entries.len() > RETRY_QUEUE_CAP {
            self.entries.pop_back();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(slot: u32) -> ApiPocHardwareDoc {
        ApiPocHardwareDoc {
            miner_key: "FEM-TEST".to_string(),
            miner_type: "FEM".to_string(),
            integrations: Default::default(),
            active_count: 0,
            total_count: 0,
            proportion: slot as f64, // marker so tests can tell docs apart
            slots: vec![],
        }
    }

    #[test]
    fn failed_slot_is_queued_and_returned_next_tick() {
        // Repro of the v0.4.7 drop: before the queue existed a failed slot
        // vanished; now it must come back out for the next successful tick.
        let mut q = PocRetryQueue::new();
        q.push("2026-08-10", 7, doc(7));
        assert_eq!(q.len(), 1);
        let batch = q.take_batch("2026-08-10", 12);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].slot_number, 7);
        assert!(q.is_empty());
    }

    #[test]
    fn same_slot_is_deduplicated_newest_doc_wins() {
        let mut q = PocRetryQueue::new();
        q.push("2026-08-10", 7, doc(1));
        q.push("2026-08-10", 7, doc(2));
        assert_eq!(q.len(), 1);
        let batch = q.take_batch("2026-08-10", 12);
        assert_eq!(batch[0].doc.proportion, 2.0);
    }

    #[test]
    fn cap_evicts_oldest_entries() {
        let mut q = PocRetryQueue::new();
        for slot in 0..(RETRY_QUEUE_CAP as u32 + 10) {
            // Distinct dates so dedupe can't hide the overflow.
            q.push(&format!("2026-08-{:02}", slot % 28 + 1), slot, doc(slot));
        }
        assert_eq!(q.len(), RETRY_QUEUE_CAP);
    }

    #[test]
    fn stale_days_are_discarded_on_take() {
        let mut q = PocRetryQueue::new();
        q.push("2026-08-09", 143, doc(143));
        q.push("2026-08-10", 0, doc(0));
        let batch = q.take_batch("2026-08-10", 12);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].slot_number, 0);
        assert!(q.is_empty(), "yesterday's entry must be dropped, not kept");
    }

    #[test]
    fn take_batch_is_bounded_and_oldest_first() {
        let mut q = PocRetryQueue::new();
        for slot in 0..5 {
            q.push("2026-08-10", slot, doc(slot));
        }
        let batch = q.take_batch("2026-08-10", 2);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].slot_number, 0);
        assert_eq!(batch[1].slot_number, 1);
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn requeue_puts_the_entry_back_at_the_front() {
        let mut q = PocRetryQueue::new();
        q.push("2026-08-10", 3, doc(3));
        q.push("2026-08-10", 4, doc(4));
        let mut batch = q.take_batch("2026-08-10", 1);
        let failed = batch.remove(0);
        q.requeue(failed);
        let batch = q.take_batch("2026-08-10", 2);
        assert_eq!(batch[0].slot_number, 3, "requeued entry stays oldest-first");
        assert_eq!(batch[1].slot_number, 4);
    }
}
