pub mod scrubber;

use std::path::Path;

/// Initialize logging with rotating files + scrubbing (10 files × 5MB).
/// Keeps stdout in dev mode, uses file in release.
///
/// Scrubber redacts:
/// - 25-word BIP39 mnemonics → [MNEMONIC]
/// - bearer/api_key/token/OP_* → [REDACTED]
/// - 58-char base32 Algorand addresses → first4…last4
/// - IPv4 → mask last octet
/// - MACs → [MAC]
/// - Usernames → <user>
/// - Hostnames → <host>
/// - Serial-like long hex → [SERIAL]
pub fn init_logging(log_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::fmt::format::FmtSpan;

    #[cfg(debug_assertions)]
    {
        // Dev: stdout only
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_span_events(FmtSpan::CLOSE)
            .init();
        return Ok(());
    }

    #[cfg(not(debug_assertions))]
    {
        // Release: rotating files with scrubbing
        std::fs::create_dir_all(log_dir)?;

        let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::FILES_PER_DAY)
            .max_bytes(5_000_000) // 5 MB per file
            .max_backups(10)
            .build(log_dir)?;

        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_writer(non_blocking)
            .with_span_events(FmtSpan::CLOSE)
            .init();

        Ok(())
    }
}
