use anyhow::{anyhow, Result};
use rand::rngs::OsRng;
use rand::RngCore;

const PREFIX: &str = "FEM";
const HEX_LEN: usize = 32;

/// Generate a new FEM miner key: "FEM-" + 32 lowercase hex chars (16 random bytes)
pub fn generate() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!("{}-{}", PREFIX, hex::encode(bytes))
}

/// Parse a miner key into (prefix, hex) components, validating format
pub fn parse(key: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = key.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err(anyhow!("Invalid miner key format: missing separator"));
    }
    let prefix = parts[0];
    let hex_part = parts[1];
    if prefix != PREFIX {
        return Err(anyhow!(
            "Invalid prefix: expected '{}', got '{}'",
            PREFIX,
            prefix
        ));
    }
    if hex_part.len() != HEX_LEN {
        return Err(anyhow!(
            "Invalid hex length: expected {}, got {}",
            HEX_LEN,
            hex_part.len()
        ));
    }
    hex::decode(hex_part).map_err(|e| anyhow!("Invalid hex: {}", e))?;
    Ok((prefix.to_string(), hex_part.to_string()))
}

pub fn is_valid(key: &str) -> bool {
    parse(key).is_ok()
}

/// Normalize a FEM key: trim whitespace, require uppercase FEM prefix,
/// require exactly 32 hex chars, return FEM- + lowercase hex.
pub fn normalize_fem_key(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let parts: Vec<&str> = trimmed.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err("Invalid FEM key format: missing separator".to_string());
    }
    let prefix = parts[0];
    let hex_part = parts[1];
    if prefix != PREFIX {
        return Err(format!(
            "Invalid FEM key format: expected prefix '{}', got '{}'",
            PREFIX, prefix
        ));
    }
    if hex_part.len() != HEX_LEN {
        return Err(format!(
            "Invalid FEM key format: expected {} hex chars, got {}",
            HEX_LEN, hex_part.len()
        ));
    }
    if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid FEM key format: non-hex character in key".to_string());
    }
    Ok(format!("{}-{}", PREFIX, hex_part.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_format() {
        let key = generate();
        assert!(key.starts_with("FEM-"));
        assert_eq!(key.len(), 4 + 32); // "FEM-" + 32 hex
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
        let (prefix, hex_part) = parse(&key).unwrap();
        assert_eq!(prefix, "FEM");
        assert_eq!(hex_part.len(), 32);
    }

    #[test]
    fn test_parse_invalid_prefix() {
        assert!(parse("BM-abcdef0123456789abcdef0123456789").is_err());
    }

    #[test]
    fn test_parse_invalid_hex() {
        assert!(parse("FEM-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
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
        assert_eq!(out, "FEM-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    }

    #[test]
    fn test_normalize_fem_key_uppercase_hex() {
        let key = "FEM-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let out = normalize_fem_key(key).unwrap();
        assert_eq!(out, "FEM-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    }

    #[test]
    fn test_normalize_fem_key_rejects_lowercase_prefix() {
        assert!(normalize_fem_key("fem-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_err());
    }

    #[test]
    fn test_normalize_fem_key_rejects_short() {
        assert!(normalize_fem_key("FEM-short").is_err());
    }

    #[test]
    fn test_normalize_fem_key_rejects_non_hex() {
        assert!(normalize_fem_key("FEM-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn test_normalize_fem_key_rejects_missing_separator() {
        assert!(normalize_fem_key("FEMaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_err());
    }
}
