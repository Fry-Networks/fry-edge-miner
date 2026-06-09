/// Hardware probing stubs for Phase 4 AEM integration.
/// GPS, audio, and sensor data collection will be implemented
/// when AEM hardware probing is ready.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsData {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFingerprint {
    pub hash: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorData {
    pub temperature: Option<f64>,
    pub humidity: Option<f64>,
}

/// Stub: returns mock GPS data
pub fn probe_gps() -> Option<GpsData> {
    None // Phase 4: real GPS probing
}

/// Stub: returns mock audio fingerprint
pub fn probe_audio() -> Option<AudioFingerprint> {
    None // Phase 4: real audio capture + fingerprinting
}

/// Stub: returns mock sensor data
pub fn probe_sensors() -> Option<SensorData> {
    None // Phase 4: real sensor reading
}
