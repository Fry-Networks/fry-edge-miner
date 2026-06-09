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
}
