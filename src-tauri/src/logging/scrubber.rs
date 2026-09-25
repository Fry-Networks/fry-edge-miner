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

    // B23: the eleven rules above miss every remaining shape the Done-when
    // names. These are appended at the END so no existing rule's output can
    // shift, and each one refuses to touch a value that is already a redaction
    // marker — which is what keeps `scrub_line` idempotent and keeps the
    // existing expectations (`api_key=[REDACTED]` and friends) byte-identical.
    result = redact_json_secret(&result);
    result = redact_named_secret(&result);
    result = redact_identity_fields(&result);
    result = redact_wireguard_key(&result);
    result = redact_loose_mnemonic(&result);
    result = redact_literal_identity(&result);
    // Row 4: last, like the B23 rules, so no earlier rule's output shifts.
    result = redact_miner_key(&result);

    result
}

/// B23: what gets scrubbed out of a PARTNER's own stdout/stderr on the way to
/// disk, as opposed to what gets scrubbed out of an exported bundle.
///
/// The two are deliberately different, and the difference is the whole point of
/// having both:
///
/// * A partner's log file is written to `%LOCALAPPDATA%\com.frynetworks.fem\
///   logs\<id>\`, whose OWN absolute path contains the Windows username. No
///   amount of content scrubbing can make that folder username-free, so trying
///   to strip usernames out of the CONTENT buys nothing there.
/// * The artifact that actually leaves the machine is the exported bundle, and
///   `commands::debug` already scrubs every collected file line-by-line with the
///   full `scrub_line` at collection time. Usernames, IPs, MACs and serials are
///   handled there, on the thing users post publicly.
///
/// So write-time scrubbing covers exactly the classes that must never reach the
/// disk in the first place because they are IRREVERSIBLE if seen: a wallet
/// address, a WireGuard key, a mnemonic, a token. The shipped frynode.exe
/// prints `Node address: <58-char Algorand address>` and `WG public key: <key>`
/// on startup — confirmed in the Go source and in the vendored binary's string
/// table — and those are the reason this function exists.
///
/// It deliberately does NOT rewrite paths. `redact_username`,
/// `redact_literal_identity`, `redact_ipv4`, `redact_mac` and `redact_serial`
/// all rewrite text that can legitimately be part of a filesystem path, and a
/// partner's output is full of paths that FEM and its own tests read back.
/// Rewriting them destroys diagnostic value and breaks real invariants — the
/// pre-existing `bug9_working_dir_tests` spawns a child, reads the path it
/// reports out of this very log, and canonicalizes it, which cannot survive a
/// substituted username because `canonicalize` requires the path to exist.
/// That test encodes a real invariant (a partner must run in the directory it
/// was given) and is correct to fail if this function mangles a path.
pub fn scrub_partner_line(line: &str) -> String {
    let mut result = line.to_string();

    // Mnemonics, both the strict 25-word form and the loose comma/mixed-case one.
    result = redact_mnemonic(&result);
    result = redact_loose_mnemonic(&result);

    // Tokens and named secrets, in every shape.
    result = redact_bearer_token(&result);
    result = redact_api_key(&result);
    result = redact_token(&result);
    result = redact_op_session(&result);
    result = redact_json_secret(&result);
    result = redact_named_secret(&result);

    // The two frynode actually prints.
    result = redact_algorand_address(&result);
    result = redact_wireguard_key(&result);

    // BUG LOOP 2: no partner is handed the miner key today, but a line that
    // carries one must not reach the disk either.
    result = redact_miner_key(&result);

    result
}

/// The markers this module itself produces. A rule that re-wrote one of these
/// would change `scrub_line`'s output on a second pass — the debug bundle
/// scrubs a second time on the way into the zip, and `redact_named_secret`
/// would otherwise overwrite `api_key=[REDACTED]` with its own marker and break
/// what the existing tests assert.
fn is_redaction_marker(value: &str) -> bool {
    matches!(
        value.trim_matches('"'),
        "<redacted>"
            | "[REDACTED]"
            | "[MNEMONIC]"
            | "[SERIAL]"
            | "[MAC]"
            | "[WGKEY]"
            | "<host>"
            | "<user>"
    )
}

/// Secret NAMES, not secret SHAPES. A value is unguessable by definition; the
/// key next to it is not.
/// Secret NAMES, not secret SHAPES. A value is unguessable by definition; the
/// key next to it is not.
///
/// The prefix class includes `.` and `-` because a real leak needed it:
/// MystNodes is started with `--user.token=<device token>` (mysterium.rs), and
/// sdk_client echoes its own invocation on an ERR/FTL line. With a `[a-z0-9_]*`
/// prefix the alternation could not span the `.` in `user.token`, so the flag
/// arm died right after `--` and the token reached the card, fem.log, the
/// partner log and the exported bundle verbatim — under a comment in
/// mysterium.rs asserting it was scrubbed.
const SECRET_NAMES: &str = r"(?:[a-z0-9_.-]*(?:mnemonic|seed[_-]?phrase|secret|passwd|password|private[_-]?key|access[_-]?key|api[_-]?key|auth[_-]?token|node[_-]?token|token))";

/// B23: `{"node_token": "<key>"}` — the exact file format `iagon.rs` tells the
/// user to create, and a shape none of the existing rules match (they need a
/// double-quoted `=` assignment or a bare `name:` form).
fn redact_json_secret(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE
        .get_or_init(|| Regex::new(&format!(r#"(?i)"({SECRET_NAMES})"\s*:\s*"([^"]*)""#)).unwrap());
    re.replace_all(s, |caps: &regex::Captures| {
        if is_redaction_marker(&caps[2]) {
            caps[0].to_string()
        } else {
            format!(r#""{}": "<redacted>""#, &caps[1])
        }
    })
    .to_string()
}

/// B23: the flag and bare-assignment shapes — `-password=…` (the literal argv
/// `pawns.rs` builds), `--api-key <value>`, and a plain `NODE_MNEMONIC=…`.
/// `redact_api_key` and `redact_token` only ever matched a double-quoted `=`
/// assignment or a `:`-separated form.
fn redact_named_secret(s: &str) -> String {
    // A FLAG may separate its value with `=` or a space (`--api-key ABC`).
    static FLAG: OnceLock<Regex> = OnceLock::new();
    let flag = FLAG.get_or_init(|| {
        Regex::new(&format!(
            r#"(?i)(^|[\s,;(\[{{])(--?)({SECRET_NAMES})\s*[= ]\s*("[^"]*"|\S+)"#
        ))
        .unwrap()
    });
    let out = flag.replace_all(s, |caps: &regex::Captures| {
        if is_redaction_marker(&caps[4]) {
            return caps[0].to_string();
        }
        format!("{}{}{}=<redacted>", &caps[1], &caps[2], &caps[3])
    });

    // A BARE assignment must have the `=`. Deliberately NOT accepting a space
    // here: "the secret is safe" would otherwise come back as
    // "the secret=<redacted> safe", and a redactor that eats prose is how a
    // log stops being worth collecting.
    static BARE: OnceLock<Regex> = OnceLock::new();
    let bare = BARE.get_or_init(|| {
        Regex::new(&format!(
            r#"(?i)(^|[\s,;(\[{{])({SECRET_NAMES})\s*=\s*("[^"]*"|\S+)"#
        ))
        .unwrap()
    });
    let out = bare.replace_all(&out, |caps: &regex::Captures| {
        if is_redaction_marker(&caps[3]) {
            return caps[0].to_string();
        }
        format!("{}{}=<redacted>", &caps[1], &caps[2])
    });

    // A COLON form. Only `token:` and `api[_-]key:` had a colon rule, so
    // `password: hunter2`, `secret: x`, `private_key: x` and `user.token: x`
    // all came back unredacted. Requires a space or quote after the colon so a
    // Windows path (`C:\Users\x`) and a URL scheme cannot match.
    static COLON: OnceLock<Regex> = OnceLock::new();
    let colon = COLON.get_or_init(|| {
        Regex::new(&format!(
            r#"(?i)(^|[\s,;(\[{{])(--?)?({SECRET_NAMES})\s*:\s+("[^"]*"|\S+)"#
        ))
        .unwrap()
    });
    colon
        .replace_all(&out, |caps: &regex::Captures| {
            if is_redaction_marker(&caps[4]) {
                return caps[0].to_string();
            }
            format!(
                "{}{}{}=<redacted>",
                &caps[1],
                caps.get(2).map_or("", |m| m.as_str()),
                &caps[3]
            )
        })
        .to_string()
}

/// B23: `device_name=GEORGE-RIG-01 device_id=fem-george-rig-01` — the exact
/// shape `pawns.rs` emits. `redact_hostname` keys on the literal word
/// `hostname`, so none of this was matched. That rule STAYS; this generalises
/// it without replacing it.
fn redact_identity_fields(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)\b(device_name|device_id|computer_?name|user_?name|host)\s*=\s*(\S+)")
            .unwrap()
    });
    re.replace_all(s, |caps: &regex::Captures| {
        if is_redaction_marker(&caps[2]) {
            caps[0].to_string()
        } else {
            format!("{}=<redacted>", &caps[1])
        }
    })
    .to_string()
}

/// B23: a 44-char base64 Curve25519 key. The only base64-ish rule FEM had was
/// `redact_serial`, which is lowercase hex only, so a WireGuard key matched
/// nothing. frynode prints its PUBLIC key on startup; the private key is
/// written 0600 and never printed — this is defensive either way.
fn redact_wireguard_key(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b[A-Za-z0-9+/]{43}=").unwrap());
    re.replace_all(s, "[WGKEY]").to_string()
}

/// B23: the existing mnemonic rule needs exactly 25 lowercase space-separated
/// words on one line and has no `(?i)`. A comma-separated, mixed-case or
/// 24-word form escaped it. The original rule at the top of the pipeline is
/// untouched.
///
/// Row 4: open-ended, like the strict rule, so a phrase with prose words
/// around it is taken whole instead of leaving its last word behind.
fn redact_loose_mnemonic(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"(?i)(?:\b[a-z]{3,12}\b[ ,]+){23,}[a-z]{3,12}\b").unwrap());
    re.replace_all(s, |caps: &regex::Captures| {
        mnemonic_replacement(s, &caps[0], caps.get(0).map_or(0, |m| m.end()))
    })
    .to_string()
}

/// BUG LOOP 2: what a mnemonic run is replaced with. Open-ended runs can end
/// in a secret NAME whose value follows it (`… adapt password=X`,
/// `… token: X`); swallowing the name hid the value from every name-keyed
/// rule that runs after the mnemonic rules. That last word is kept — but only
/// when it IS a secret name AND a value is assigned to it, because "secret"
/// and "token" are phrase words too.
fn mnemonic_replacement(line: &str, run: &str, end: usize) -> String {
    static NAME: OnceLock<Regex> = OnceLock::new();
    let name = NAME.get_or_init(|| Regex::new(&format!(r"(?i)^{SECRET_NAMES}$")).unwrap());
    // BUG LOOP 3: a JOINED name (`api-key`, `user.token`, `seed-phrase`)
    // continues past the run: a run's last word stops at '.' or '-', so that
    // word is only the name's first segment and the rest follows the match.
    let rest = &line[end..];
    let joined = rest.len()
        - rest
            .trim_start_matches(|c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .len();
    let (tail, after) = rest.split_at(joined);
    let assigned = after.trim_start().starts_with(['=', ':']);
    // BUG LOOP 3: the separator is found WITH its width. The strict rule's
    // `\s` is Unicode, so it can be a multibyte U+00A0 or U+3000, and slicing
    // one byte past its start panicked inside it.
    match run
        .char_indices()
        .rev()
        .find(|&(_, c)| c.is_whitespace() || c == ',')
    {
        Some((cut, sep))
            if assigned && name.is_match(&format!("{}{tail}", &run[cut + sep.len_utf8()..])) =>
        {
            format!("[MNEMONIC]{}", &run[cut..])
        }
        _ => "[MNEMONIC]".to_string(),
    }
}

/// B23: the only rule that can catch a BARE `GEORGE-RIG-01` or `jdoe` in
/// arbitrary partner output — nothing else in the pipeline matches a name with
/// no surrounding structure.
///
/// Built from the real values once at startup, and pure so the tests never
/// touch process-global state.
pub(crate) fn identity_rules(
    computer_name: Option<&str>,
    user_name: Option<&str>,
) -> Vec<(Regex, &'static str)> {
    let mut rules = Vec::new();
    for (value, marker) in [(computer_name, "<host>"), (user_name, "<user>")] {
        let Some(value) = value.map(str::trim).filter(|v| v.len() >= 3) else {
            continue;
        };
        // Word-bounded and case-insensitive: Windows reports the same name in
        // several cases, and a substring match would eat unrelated text.
        if let Ok(re) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(value))) {
            rules.push((re, marker));
        }
    }
    rules
}

pub(crate) fn redact_literals(s: &str, rules: &[(Regex, &'static str)]) -> String {
    let mut out = s.to_string();
    for (re, marker) in rules {
        out = re.replace_all(&out, *marker).to_string();
    }
    out
}

static IDENTITY: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();

/// Seed the literal-identity rules. Called once from `logging::init_logging`
/// before any layer exists; a no-op afterwards. Until it runs, the rule matches
/// nothing, so nothing before it can be over-redacted.
pub fn seed_identity(computer_name: Option<&str>, user_name: Option<&str>) {
    let _ = IDENTITY.set(identity_rules(computer_name, user_name));
}

fn redact_literal_identity(s: &str) -> String {
    match IDENTITY.get() {
        Some(rules) if !rules.is_empty() => redact_literals(s, rules),
        _ => s.to_string(),
    }
}

/// Redact 25-word BIP39 mnemonic sequences
///
/// Row 4: 25 words OR MORE. Exactly 25 matched from the first word of a run,
/// so a phrase with a prose word in front of it ("loose abandon … adapt")
/// lost 25 words from the front and left its real last word on the line —
/// and this rule runs first, so nothing after it could see the phrase whole.
fn redact_mnemonic(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?:\b[a-z]{3,12}\b\s+){24,}[a-z]{3,12}\b").unwrap());
    re.replace_all(s, |caps: &regex::Captures| {
        mnemonic_replacement(s, &caps[0], caps.get(0).map_or(0, |m| m.end()))
    })
    .to_string()
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

/// Row 4: a miner key. The M6 bundle carried the device's key in plain text
/// (`miner_key=FEM-<UPPERCASE HEX>`): the only rule that could touch it,
/// `redact_serial`, matches lowercase hex only.
///
/// BUG LOOP 2: two rules, because the product both ACCEPTS and KEEPS more
/// than hex. The SHAPE is the validator's own contract (`config/miner_key.rs`:
/// `FEM-` + 32 alphanumerics, any case), redacted wherever it appears —
/// including the request URLs FEM builds from it. The FIELD rule redacts
/// whatever a `miner_key` field holds, because `resolve_stored_miner_key`
/// keeps a stored key that fails validation (e.g. `FEM-SHORTKEY123`) and the
/// device emitters log it as is: `miner_key=…` / `miner_key: …` (tracing,
/// quoted or not) and `"miner_key": "…"` (JSON).
fn redact_miner_key(s: &str) -> String {
    static SHAPE: OnceLock<Regex> = OnceLock::new();
    let shape = SHAPE.get_or_init(|| Regex::new(r"(?i)\bFEM-[0-9A-Z]{32}\b").unwrap());
    let out = shape.replace_all(s, "FEM-[REDACTED]");

    static FIELD: OnceLock<Regex> = OnceLock::new();
    let field = FIELD.get_or_init(|| {
        Regex::new(r#"(?i)(\bminer_key\s*[=:]\s*|"miner_key"\s*:\s*)("[^"]*"|[^\s,}]+)"#).unwrap()
    });
    let out = field.replace_all(&out, |caps: &regex::Captures| {
        if caps[2].starts_with('"') {
            format!(r#"{}"FEM-[REDACTED]""#, &caps[1])
        } else {
            format!("{}FEM-[REDACTED]", &caps[1])
        }
    });

    // BUG LOOP 3: the kept key, whatever its length, also rides in the request
    // paths FEM builds from it (`/credentials/{key}`, `/PoC/{key}/hardware`,
    // `/installations/{key}/…`), which reach logs inside reqwest and decode
    // errors with no `miner_key` field beside them. A `FEM-` segment is taken
    // as the key only inside an http(s) URL or right after one of those
    // routes: a filesystem path is never rewritten (partner logs depend on
    // that), and `FEM-` names such as the firewall rules are left alone.
    static PATH: OnceLock<Regex> = OnceLock::new();
    let path = PATH.get_or_init(|| {
        Regex::new(
            r#"(?i)(https?://[^\s)"']*?/|/(?:credentials|installations|poc)/)FEM-[^/?#\s)"'\\]+"#,
        )
        .unwrap()
    });
    let out = path.replace_all(&out, |caps: &regex::Captures| {
        format!("{}FEM-[REDACTED]", &caps[1])
    });

    // …and in a Debug-printed body, whose JSON quotes are escaped:
    // `\"miner_key\": \"…\"`.
    static ESCAPED: OnceLock<Regex> = OnceLock::new();
    let escaped =
        ESCAPED.get_or_init(|| Regex::new(r#"(?i)(\\"miner_key\\"\s*:\s*)\\"[^"\\]*\\""#).unwrap());
    escaped
        .replace_all(&out, |caps: &regex::Captures| {
            format!(r#"{}\"FEM-[REDACTED]\""#, &caps[1])
        })
        .to_string()
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

/// Row 4 (continuation #4): the miner key and the tail word of a
/// prose-prefixed phrase, pinned with the run-#3 M6 inputs.
#[cfg(test)]
#[path = "scrubber_c4_tests.rs"]
mod scrubber_c4_tests;

/// BUG LOOP 2: the miner key in every shape the product accepts or keeps.
#[cfg(test)]
#[path = "scrubber_bl2_key_tests.rs"]
mod scrubber_bl2_key_tests;

/// BUG LOOP 2: a secret named right after a phrase keeps its value redacted.
#[cfg(test)]
#[path = "scrubber_bl2_name_tests.rs"]
mod scrubber_bl2_name_tests;

/// BUG LOOP 3: a multibyte Unicode separator never panics the scrubber.
#[cfg(test)]
#[path = "scrubber_bl3_unicode_tests.rs"]
mod scrubber_bl3_unicode_tests;

/// BUG LOOP 3: a kept legacy miner key in URL paths and escaped JSON.
#[cfg(test)]
#[path = "scrubber_bl3_url_key_tests.rs"]
mod scrubber_bl3_url_key_tests;

/// BUG LOOP 3: a joined secret name right after a phrase keeps its value
/// redacted.
#[cfg(test)]
#[path = "scrubber_bl3_joined_name_tests.rs"]
mod scrubber_bl3_joined_name_tests;
