use anyhow::{anyhow, Result};

const ALGORAND_ADDRESS_LEN: usize = 58;

/// Validate an Algorand wallet address (58-char base32)
pub fn validate_address(address: &str) -> Result<()> {
    if address.len() != ALGORAND_ADDRESS_LEN {
        return Err(anyhow!(
            "Invalid address length: expected {}, got {}",
            ALGORAND_ADDRESS_LEN,
            address.len()
        ));
    }
    if !address
        .chars()
        .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c))
    {
        return Err(anyhow!("Invalid address: contains non-base32 characters"));
    }
    Ok(())
}

pub fn is_valid_address(address: &str) -> bool {
    validate_address(address).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_address() {
        // 58 chars of valid base32
        let addr = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA4";
        // Actually needs to be exactly 58 valid base32 chars
        let addr = "A".repeat(58);
        assert!(validate_address(&addr).is_ok());
    }

    #[test]
    fn test_invalid_length() {
        assert!(validate_address("TOOSHORT").is_err());
    }

    #[test]
    fn test_invalid_chars() {
        let addr = "a".repeat(58); // lowercase not valid base32
        assert!(validate_address(&addr).is_err());
    }
}
