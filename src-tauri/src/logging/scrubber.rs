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
    let re = RE.get_or_init(|| Regex::new(r"(?:\b[a-z]{3,12}\b\s+){24}[a-z]{3,12}\b").unwrap());
    re.replace_all(s, "[MNEMONIC]").to_string()
}

/// Redact bearer tokens (bearer="..." or Bearer: ...)
fn redact_bearer_token(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"(?i)bearer[\s:=]+"?[^"\s]+"?"#).unwrap());
    re.replace_all(s, "bearer=[REDACTED]").to_string()
}

/// Redact api_key values
fn redact_api_key(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r#"(?i)api_key\s*=\s*"[^"]*"|api[_-]key:\s*\S+"#).unwrap());
    re.replace_all(s, "api_key=[REDACTED]").to_string()
}

/// Redact generic token values
fn redact_token(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"(?i)token\s*=\s*"[^"]*"|token:\s*\S+"#).unwrap());
    re.replace_all(s, "token=[REDACTED]").to_string()
}

/// Redact OP_SESSION and OP_* environment variables
fn redact_op_session(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"OP_[A-Za-z0-9_]+\s*=\s*\S+").unwrap());
    re.replace_all(s, "[REDACTED]").to_string()
}

/// Redact 58-char base32 Algorand addresses as first4…last4
fn redact_algorand_address(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b([A-Z2-7]{58})\b").unwrap());
    re.replace_all(s, |caps: &regex::Captures| {
        let addr = &caps[1];
        if addr.len() >= 8 {
            format!("{}…{}", &addr[..4], &addr[addr.len() - 4..])
        } else {
            addr.to_string()
        }
    })
    .to_string()
}

/// Redact IPv4 addresses (mask last octet)
fn redact_ipv4(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").unwrap());
    re.replace_all(s, |caps: &regex::Captures| {
        format!("{}.{}.{}.x", &caps[1], &caps[2], &caps[3])
    })
    .to_string()
}

/// Redact MAC addresses
fn redact_mac(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)\b([0-9a-f]{2}[:-]){5}([0-9a-f]{2})\b").unwrap());
    re.replace_all(s, "[MAC]").to_string()
}

/// Redact Windows usernames (basic heuristic)
fn redact_username(s: &str) -> String {
    // Redact patterns like C:\Users\username\...
    //
    // This used to hardcode `C:` and a backslash separator, so a profile
    // relocated to another drive leaked the username into an exported debug
    // bundle, and so did any forward-slash path (this crate builds those —
    // e.g. `download.rs` uses `C:/ProgramData`). Match any drive and either
    // separator instead.
    //
    // The drive letter is PRESERVED in the replacement: it is useful when
    // reading a bundle and identifies nobody. `${1}` rather than `$1` because
    // the next character is `:`, which would otherwise be read as part of a
    // capture-group name.
    // The separator is `+`, not a single character, because `tracing`'s fmt
    // layer ESCAPES backslashes inside field values: a spawn event reaches disk
    // as `command="C:\\Users\\name\\..."`. The single-separator pattern skipped
    // every one of those, which is the most common way a Windows path appears
    // in a real log. Found on a live device, not by the tests above.
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)([A-Z]):[\\/]+Users[\\/]+([^\\/]+)").unwrap());
    re.replace_all(s, r"${1}:\Users\<user>").to_string()
}

/// Redact hostname (basic heuristic — any HOSTNAME= pattern)
fn redact_hostname(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)hostname\s*=\s*(\S+)").unwrap());
    re.replace_all(s, "hostname=<host>").to_string()
}

/// Redact serial-like strings (hex sequences > 12 chars or alphanumeric serials)
fn redact_serial(s: &str) -> String {
    // Redact long hex strings (> 16 chars) as SHA256 prefix(8)
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b[0-9a-f]{16,}\b").unwrap());
    re.replace_all(s, "[SERIAL]").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scrubber only matched `C:\Users\`, so a profile relocated to any
    /// other drive leaked the username into an exported debug bundle. Forward-
    /// slash paths leaked too - this crate genuinely builds them (e.g.
    /// `download.rs` uses `C:/ProgramData`).
    #[test]
    fn a_non_c_drive_user_path_is_redacted() {
        let scrubbed = scrub_line(r"Config path: D:\Users\alice\AppData\Roaming");
        assert!(
            !scrubbed.contains("alice"),
            "username leaked from a D: path: {scrubbed}"
        );
        assert!(scrubbed.contains("<user>"), "{scrubbed}");
    }

    /// Found by the live canary, not by a unit test: `tracing`'s fmt layer
    /// ESCAPES backslashes inside field values, so a spawn event lands on disk
    /// as `C:\Users\name` with a doubled separator. The single-separator
    /// pattern missed it, which meant the most common shape of a Windows path
    /// in a real log -- inside a field value -- leaked the username verbatim.
    #[test]
    fn an_escaped_backslash_user_path_is_redacted() {
        // Two backslashes per separator, exactly as the canary wrote it. The
        // first version of this test had one, passed immediately, and proved
        // nothing -- the single-separator form already worked.
        let line = r#"command="C:\\Users\\jdoe\\AppData\\Local\\app.exe""#;
        assert!(
            line.contains(r"C:\\Users"),
            "this test is only meaningful against the DOUBLED separator"
        );
        let out = scrub_line(line);
        assert!(
            !out.contains("jdoe"),
            "username must be redacted, got {out:?}"
        );
        assert!(
            out.contains("<user>"),
            "expected the <user> marker, got {out:?}"
        );
    }

    #[test]
    fn a_forward_slash_user_path_is_redacted() {
        let scrubbed = scrub_line("Config path: C:/Users/alice/AppData");
        assert!(
            !scrubbed.contains("alice"),
            "username leaked from a forward-slash path: {scrubbed}"
        );
        assert!(scrubbed.contains("<user>"), "{scrubbed}");
    }

    #[test]
    fn the_drive_letter_is_preserved_because_it_identifies_nobody() {
        let scrubbed = scrub_line(r"Config path: E:\Users\bob\x");
        assert!(
            scrubbed.contains("E:"),
            "the drive is useful for debugging and leaks nothing: {scrubbed}"
        );
        assert!(!scrubbed.contains("bob"), "{scrubbed}");
    }

    /// A redactor that eats everything would pass every one-way test above.
    /// These strings MUST survive untouched.
    #[test]
    fn strings_with_no_username_are_left_alone() {
        for line in [
            r"Install dir: C:\Program Files\Fry Edge Miner",
            r"Staged at: D:\FryEdgeMiner\partners\storj",
            "Users of this feature should read the docs",
        ] {
            assert_eq!(scrub_line(line), line, "over-redacted: {line}");
        }
    }

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
