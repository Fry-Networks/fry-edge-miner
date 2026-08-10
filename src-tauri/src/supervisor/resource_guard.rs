use std::time::Instant;

/// One resource probe of a partner process group.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// Summed working set of all processes with the target image name.
    pub mem_bytes: u64,
    /// Summed total CPU seconds consumed so far (monotonic per process
    /// lifetime; resets when the process restarts).
    pub cpu_seconds: f64,
    pub at: Instant,
}

/// Why the guard tripped.
#[derive(Debug, Clone, PartialEq)]
pub enum TripReason {
    Memory { used_bytes: u64, cap_bytes: u64 },
    Cpu { fraction: f64, cap_fraction: f64 },
}

/// Default caps (v0.4.8): a partner process may not hold more than 75% of
/// physical RAM, nor sustain more than 50% of total CPU across
/// CPU_SUSTAIN_TICKS consecutive health ticks (single spikes are fine —
/// browsers burst on startup).
pub const MEM_CAP_FRACTION: f64 = 0.75;
pub const CPU_CAP_FRACTION: f64 = 0.50;
pub const CPU_SUSTAIN_TICKS: u32 = 3;

/// Stateful breach tracker — one per guarded integration.
#[derive(Debug, Default)]
pub struct ResourceGuard {
    prev: Option<Sample>,
    consecutive_cpu_breaches: u32,
}

impl ResourceGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one sample; returns Some(reason) when the process should be
    /// restarted. `cpu_cores` scales the CPU fraction so "50%" means half of
    /// the whole machine, not half of one core.
    pub fn evaluate(
        &mut self,
        sample: Sample,
        mem_cap_bytes: u64,
        cpu_cap_fraction: f64,
        cpu_cores: u32,
    ) -> Option<TripReason> {
        // Memory breaches trip immediately — an OOM box is unusable now.
        if mem_cap_bytes > 0 && sample.mem_bytes > mem_cap_bytes {
            self.prev = Some(sample);
            self.consecutive_cpu_breaches = 0;
            return Some(TripReason::Memory {
                used_bytes: sample.mem_bytes,
                cap_bytes: mem_cap_bytes,
            });
        }

        let trip = if let Some(prev) = self.prev {
            let elapsed = sample.at.saturating_duration_since(prev.at).as_secs_f64();
            // cpu_seconds resets when the process restarts — a negative delta
            // is a restart, not a breach.
            let delta = sample.cpu_seconds - prev.cpu_seconds;
            if elapsed > 0.0 && delta > 0.0 {
                let fraction = delta / elapsed / (cpu_cores.max(1) as f64);
                if fraction > cpu_cap_fraction {
                    self.consecutive_cpu_breaches += 1;
                } else {
                    self.consecutive_cpu_breaches = 0;
                }
                if self.consecutive_cpu_breaches >= CPU_SUSTAIN_TICKS {
                    self.consecutive_cpu_breaches = 0;
                    Some(TripReason::Cpu {
                        fraction,
                        cap_fraction: cpu_cap_fraction,
                    })
                } else {
                    None
                }
            } else {
                self.consecutive_cpu_breaches = 0;
                None
            }
        } else {
            None
        };

        self.prev = Some(sample);
        trip
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn s(base: Instant, secs: u64, mem: u64, cpu: f64) -> Sample {
        Sample {
            mem_bytes: mem,
            cpu_seconds: cpu,
            at: base + Duration::from_secs(secs),
        }
    }

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn memory_over_cap_trips_immediately() {
        // Repro of the field report: OlostepBrowser consuming nearly all RAM
        // ran unbounded — nothing watched it.
        let mut g = ResourceGuard::new();
        let base = Instant::now();
        let trip = g.evaluate(s(base, 0, 13 * GB, 1.0), 12 * GB, CPU_CAP_FRACTION, 8);
        assert_eq!(
            trip,
            Some(TripReason::Memory {
                used_bytes: 13 * GB,
                cap_bytes: 12 * GB
            })
        );
    }

    #[test]
    fn cpu_spike_for_one_tick_does_not_trip() {
        let mut g = ResourceGuard::new();
        let base = Instant::now();
        // 8 cores; 30s tick; 240 cpu-seconds = 100% of the whole machine.
        assert_eq!(g.evaluate(s(base, 0, GB, 0.0), 12 * GB, 0.5, 8), None);
        assert_eq!(g.evaluate(s(base, 30, GB, 240.0), 12 * GB, 0.5, 8), None);
        // Then it calms down — counter must reset.
        assert_eq!(g.evaluate(s(base, 60, GB, 241.0), 12 * GB, 0.5, 8), None);
        assert_eq!(g.evaluate(s(base, 90, GB, 480.0), 12 * GB, 0.5, 8), None);
    }

    #[test]
    fn sustained_cpu_breach_trips_on_the_third_tick() {
        let mut g = ResourceGuard::new();
        let base = Instant::now();
        assert_eq!(g.evaluate(s(base, 0, GB, 0.0), 12 * GB, 0.5, 8), None);
        // ~100% of machine for 3 consecutive deltas
        assert_eq!(g.evaluate(s(base, 30, GB, 240.0), 12 * GB, 0.5, 8), None);
        assert_eq!(g.evaluate(s(base, 60, GB, 480.0), 12 * GB, 0.5, 8), None);
        let trip = g.evaluate(s(base, 90, GB, 720.0), 12 * GB, 0.5, 8);
        match trip {
            Some(TripReason::Cpu { fraction, .. }) => assert!(fraction > 0.5, "fraction={fraction}"),
            other => panic!("expected CPU trip, got {other:?}"),
        }
    }

    #[test]
    fn process_restart_resets_instead_of_tripping() {
        // cpu_seconds drops when the process restarts — negative delta.
        let mut g = ResourceGuard::new();
        let base = Instant::now();
        assert_eq!(g.evaluate(s(base, 0, GB, 500.0), 12 * GB, 0.5, 8), None);
        assert_eq!(g.evaluate(s(base, 30, GB, 2.0), 12 * GB, 0.5, 8), None);
    }

    #[test]
    fn zero_mem_cap_disables_the_memory_check() {
        let mut g = ResourceGuard::new();
        let base = Instant::now();
        assert_eq!(g.evaluate(s(base, 0, 13 * GB, 1.0), 0, 0.5, 8), None);
    }
}
