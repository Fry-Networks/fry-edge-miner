use regex::Regex;
use std::sync::OnceLock;

/// Redact sensitive data before writing to logs.
///
/// Handles: mnemonics, API keys/tokens, Algorand addresses, IPs, MACs, usernames, hostnames, serials.
pub fn scrub_line(line: &str) -> String {
    let mut result = line.to_string();

    // Redact 25-word BIP39 mnemonics (25 words separated by spaces, each 3-12 chars alphabetic)
    result = redact_mnemonic(&result);

    // Redact bearer/api_key/token/OP_SESSION/OP_* values
    result = redact_bearer_token(&result);
    result = redact_api_key(&result);
    result = redact_token(&result);
    result = redact_op_session(&result);

    // Redact 58-char base32 Algorand addresses (display as first4…last4)
    result = redact_algorand_address(&result);

    // Redact IPv4 addresses (mask last octet)
    result = redact_ipv4(&result);

    // Redact MAC addresses
    result = redact_mac(&result);

    // Redact Windows username (from COMPUTERNAME or %USERNAME%)
    result = redact_username(&result);

    // Redact hostname
    result = redact_hostname(&result);

    // Redact serial-like strings (long hex or alphanumeric sequences)
    result = redact_serial(&result);

    result
}

/// Redact 25-word BIP39 mnemonic sequences
fn redact_mnemonic(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?:\b[a-z]{3,12}\b\s+){24}[a-z]{3,12}\b").unwrap()
    });
    re.replace_all(s, "[MNEMONIC]").to_string()
}

/// Redact bearer tokens (bearer="..." or Bearer: ...)
fn redact_bearer_token(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?i)bearer[\s:=]+"?[^"\s]+"?"#).unwrap()
    });
    re.replace_all(s, "bearer=[REDACTED]").to_string()
}

/// Redact api_key values
fn redact_api_key(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?i)api_key\s*=\s*"[^"]*"|api[_-]key:\s*\S+"#).unwrap()
    });
    re.replace_all(s, "api_key=[REDACTED]").to_string()
}

/// Redact generic token values
fn redact_token(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?i)token\s*=\s*"[^"]*"|token:\s*\S+"#).unwrap()
    });
    re.replace_all(s, "token=[REDACTED]").to_string()
}

/// Redact OP_SESSION and OP_* environment variables
fn redact_op_session(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"OP_[A-Za-z0-9_]+\s*=\s*\S+").unwrap()
    });
    re.replace_all(s, "[REDACTED]").to_string()
}

/// Redact 58-char base32 Algorand addresses as first4…last4
fn redact_algorand_address(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\b([A-Z2-7]{58})\b").unwrap()
    });
    re.replace_all(s, |caps: &regex::Captures| {
        let addr = &caps[1];
        if addr.len() >= 8 {
            format!("{}…{}", &addr[..4], &addr[addr.len()-4..])
        } else {
            addr.to_string()
        }
    }).to_string()
}

/// Redact IPv4 addresses (mask last octet)
fn redact_ipv4(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").unwrap()
    });
    re.replace_all(s, |caps: &regex::Captures| {
        format!("{}.{}.{}.x", &caps[1], &caps[2], &caps[3])
    }).to_string()
}

/// Redact MAC addresses
fn redact_mac(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)\b([0-9a-f]{2}[:-]){5}([0-9a-f]{2})\b").unwrap()
    });
    re.replace_all(s, "[MAC]").to_string()
}

/// Redact Windows usernames (basic heuristic)
fn redact_username(s: &str) -> String {
    // Redact patterns like C:\Users\username\...
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)C:\\Users\\([^\\/]+)").unwrap()
    });
    re.replace_all(s, r"C:\Users\<user>").to_string()
}

/// Redact hostname (basic heuristic — any HOSTNAME= pattern)
fn redact_hostname(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)hostname\s*=\s*(\S+)").unwrap()
    });
    re.replace_all(s, "hostname=<host>").to_string()
}

/// Redact serial-like strings (hex sequences > 12 chars or alphanumeric serials)
fn redact_serial(s: &str) -> String {
    // Redact long hex strings (> 16 chars) as SHA256 prefix(8)
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\b[0-9a-f]{16,}\b").unwrap()
    });
    re.replace_all(s, "[SERIAL]").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_mnemonic() {
        let mnemonic = "ability abstract abstract abstract abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        let scrubbed = scrub_line(mnemonic);
        assert!(scrubbed.contains("[MNEMONIC]"));
        assert!(!scrubbed.contains("ability"));
    }

    #[test]
    fn test_scrub_bearer_token() {
        let line = r#"Authorization: Bearer sk-test123abc456def789"#;
        let scrubbed = scrub_line(line);
        assert!(scrubbed.contains("[REDACTED]"));
        assert!(!scrubbed.contains("sk-test"));
    }

    #[test]
    fn test_scrub_api_key() {
        let line = r#"api_key = "secret-key-12345""#;
        let scrubbed = scrub_line(line);
        assert!(scrubbed.contains("[REDACTED]"));
        assert!(!scrubbed.contains("secret-key"));
    }

    #[test]
    fn test_scrub_algorand_address() {
        let addr = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let line = format!("wallet: {}", addr);
        let scrubbed = scrub_line(&line);
        assert!(scrubbed.contains("AAAA…AAAA"));
        assert!(!scrubbed.contains(addr));
    }

    #[test]
    fn test_scrub_ipv4() {
        let line = "Connected to 192.168.1.100";
        let scrubbed = scrub_line(line);
        assert!(scrubbed.contains("192.168.1.x"));
    }

    #[test]
    fn test_scrub_mac() {
        let line = "MAC address: 00:11:22:33:44:55";
        let scrubbed = scrub_line(line);
        assert!(scrubbed.contains("[MAC]"));
        assert!(!scrubbed.contains("00:11:22"));
    }

    #[test]
    fn test_scrub_windows_path() {
        let line = r"Config path: C:\Users\alice\AppData";
        let scrubbed = scrub_line(line);
        assert!(scrubbed.contains(r"C:\Users\<user>"));
        assert!(!scrubbed.contains("alice"));
    }

    #[test]
    fn test_scrub_op_session() {
        let line = "OP_SESSION_frynetworks=abc123xyz789";
        let scrubbed = scrub_line(line);
        assert!(scrubbed.contains("[REDACTED]"));
        assert!(!scrubbed.contains("abc123"));
    }
}
