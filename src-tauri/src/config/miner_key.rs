use anyhow::{anyhow, Result};
use rand::rngs::OsRng;
use rand::RngCore;

const PREFIX: &str = "FEM";
const HEX_LEN: usize = 32;

/// Generate a new FEM miner key: "FEM-" + 32 lowercase hex chars (16 random bytes).
/// Generated keys are hex-only, but user-provided keys may be any 32 alphanumeric characters.
pub fn generate() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!("{}-{}", PREFIX, hex::encode(bytes))
}

/// Parse a miner key into (prefix, body) components, validating format.
/// Accepts exactly 32 alphanumeric characters after the FEM- prefix.
#[allow(dead_code)] // key-format validation utility, unit-tested, kept for API parity
pub fn parse(key: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = key.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err(anyhow!("Invalid miner key format: missing separator"));
    }
    let prefix = parts[0];
    let key_part = parts[1];
    if prefix != PREFIX {
        return Err(anyhow!(
            "Invalid prefix: expected '{}', got '{}'",
            PREFIX,
            prefix
        ));
    }
    if key_part.len() != HEX_LEN {
        return Err(anyhow!(
            "Invalid key length: expected {}, got {}",
            HEX_LEN,
            key_part.len()
        ));
    }
    if !key_part.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(anyhow!("Invalid key body: must contain only alphanumeric characters"));
    }
    Ok((prefix.to_string(), key_part.to_string()))
}

#[allow(dead_code)] // key-format validation utility, unit-tested, kept for API parity
pub fn is_valid(key: &str) -> bool {
    parse(key).is_ok()
}

/// Normalize a FEM key: trim whitespace, require FEM- prefix (case-insensitive),
/// require exactly 32 alphanumeric characters, return FEM- + uppercase body.
pub fn normalize_fem_key(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let parts: Vec<&str> = trimmed.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err("Invalid FEM key format: missing separator".to_string());
    }
    let prefix = parts[0];
    let key_part = parts[1];
    if !prefix.eq_ignore_ascii_case(PREFIX) {
        return Err(format!(
            "Invalid FEM key format: expected prefix '{}', got '{}'",
            PREFIX, prefix
        ));
    }
    if key_part.len() != HEX_LEN {
        return Err(format!(
            "Invalid FEM key format: expected {} alphanumeric chars, got {}",
            HEX_LEN, key_part.len()
        ));
    }
    if !key_part.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Invalid FEM key format: non-alphanumeric character in key".to_string());
    }
    Ok(format!("{}-{}", PREFIX, key_part.to_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_format() {
        let key = generate();
        assert!(key.starts_with("FEM-"));
        assert_eq!(key.len(), 4 + 32); // "FEM-" + 32 chars
    }

    #[test]
    fn test_generate_unique() {
        let a = generate();
        let b = generate();
        assert_ne!(a, b);
    }

    #[test]
    fn test_parse_valid() {
        let key = generate();
        let (prefix, key_part) = parse(&key).unwrap();
        assert_eq!(prefix, "FEM");
        assert_eq!(key_part.len(), 32);
    }

    #[test]
    fn test_parse_invalid_prefix() {
        assert!(parse("BM-abcdef0123456789abcdef0123456789").is_err());
    }

    #[test]
    fn test_parse_invalid_alphanumeric() {
        assert!(parse("FEM-___________notalphanumeric____________").is_err());
    }

    #[test]
    fn test_parse_wrong_length() {
        assert!(parse("FEM-abcdef").is_err());
    }

    #[test]
    fn test_is_valid() {
        let key = generate();
        assert!(is_valid(&key));
        assert!(!is_valid("INVALID"));
        assert!(!is_valid("BM-abcdef0123456789abcdef0123456789"));
    }

    #[test]
    fn test_normalize_fem_key_trim() {
        let key = " FEM-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ";
        let out = normalize_fem_key(key).unwrap();
        assert_eq!(out, "FEM-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn test_normalize_fem_key_uppercase_body() {
        let key = "FEM-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let out = normalize_fem_key(key).unwrap();
        assert_eq!(out, "FEM-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn test_normalize_fem_key_lowercase_body() {
        let key = "FEM-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let out = normalize_fem_key(key).unwrap();
        assert_eq!(out, "FEM-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn test_normalize_fem_key_accepts_lowercase_prefix() {
        let key = "fem-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let out = normalize_fem_key(key).unwrap();
        assert_eq!(out, "FEM-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn test_normalize_fem_key_rejects_short() {
        assert!(normalize_fem_key("FEM-short").is_err());
    }

    #[test]
    fn test_normalize_fem_key_rejects_non_alphanumeric() {
        assert!(normalize_fem_key("FEM-___________notalphanumeric____________").is_err());
    }

    #[test]
    fn test_normalize_fem_key_rejects_missing_separator() {
        assert!(normalize_fem_key("FEMaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_err());
    }

    #[test]
    fn test_normalize_fem_key_real_format() {
        // Synthetic fixture matching real production key shape (uppercase alphanumeric, not hex-only)
        let key = "FEM-Z21JQG5PJG5GJIX4PRXIEONBXVXQCQ2U";
        let out = normalize_fem_key(key).unwrap();
        assert_eq!(out, "FEM-Z21JQG5PJG5GJIX4PRXIEONBXVXQCQ2U");
    }

    #[test]
    fn test_parse_real_format() {
        let key = "FEM-Z21JQG5PJG5GJIX4PRXIEONBXVXQCQ2U";
        let (prefix, key_part) = parse(key).unwrap();
        assert_eq!(prefix, "FEM");
        assert_eq!(key_part, "Z21JQG5PJG5GJIX4PRXIEONBXVXQCQ2U");
    }
}
