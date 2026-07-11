use std::error::Error as StdError;

/// Coarse classification of a failed HTTP request, used to give users an
/// actionable message instead of a raw reqwest error chain (UB-2: carriers
/// like AT&T 5G are known to block *.frynetworks.com DNS for some users).
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum RequestErrorKind {
    Dns,
    Connect,
    Timeout,
    Tls,
    Other,
}

/// Scan an error source chain for DNS / TLS markers. Split out from
/// `classify_request_error` so it can be unit-tested without constructing
/// real reqwest errors.
pub fn classify_error_chain(err: &dyn StdError) -> RequestErrorKind {
    let mut dns = false;
    let mut tls = false;
    let mut current: Option<&dyn StdError> = Some(err);
    while let Some(cause) = current {
        let msg = cause.to_string().to_lowercase();
        if msg.contains("dns")
            || msg.contains("resolve")
            || msg.contains("lookup")
            || msg.contains("name not found")
            || msg.contains("no such host")
            || msg.contains("getaddrinfo")
        {
            dns = true;
        }
        if msg.contains("certificate")
            || msg.contains("tls")
            || msg.contains("ssl")
            || msg.contains("handshake")
        {
            tls = true;
        }
        current = cause.source();
    }
    // DNS wins over TLS: a blocked resolver can surface both markers, and the
    // DNS guidance is the actionable one.
    if dns {
        RequestErrorKind::Dns
    } else if tls {
        RequestErrorKind::Tls
    } else {
        RequestErrorKind::Other
    }
}

/// Classify a reqwest error. Chain markers (DNS/TLS) take precedence over
/// `is_connect()` because reqwest reports DNS and TLS failures as connect
/// errors too.
pub fn classify_request_error(err: &reqwest::Error) -> RequestErrorKind {
    if err.is_timeout() {
        return RequestErrorKind::Timeout;
    }
    match classify_error_chain(err) {
        RequestErrorKind::Other => {
            if err.is_connect() {
                RequestErrorKind::Connect
            } else {
                RequestErrorKind::Other
            }
        }
        kind => kind,
    }
}

/// Actionable, user-facing message for each failure class. Rendered verbatim
/// in the wizard error panel (which preserves line breaks).
pub fn user_facing_message(kind: RequestErrorKind) -> Option<&'static str> {
    match kind {
        RequestErrorKind::Dns => Some(
            "Could not resolve hardwareapi.frynetworks.com — DNS lookup failed.\n\
             Some mobile carriers and DNS providers are known to block *.frynetworks.com subdomains.\n\
             • Try a different network (Wi-Fi instead of cellular hotspot)\n\
             • Set your DNS server to 1.1.1.1 or 8.8.8.8 and Retry\n\
             • If it persists, contact Fry Networks support on Discord",
        ),
        RequestErrorKind::Connect => Some(
            "Could not connect to hardwareapi.frynetworks.com.\n\
             Check your internet connection and firewall, then Retry.\n\
             If other sites work but this fails, your network may be blocking *.frynetworks.com.",
        ),
        RequestErrorKind::Timeout => Some(
            "The registration request timed out.\n\
             Check your connection and Retry. Slow or filtered networks can cause this.",
        ),
        RequestErrorKind::Tls => Some(
            "Secure connection (TLS) to hardwareapi.frynetworks.com failed.\n\
             Check your system clock is correct and that no security software is intercepting HTTPS, then Retry.",
        ),
        RequestErrorKind::Other => None,
    }
}

/// Build the registration error string shown to the user: a classified,
/// actionable headline when we recognize the failure, always followed by the
/// technical detail for support.
pub fn user_facing_registration_error(err: &reqwest::Error) -> String {
    let kind = classify_request_error(err);
    let mut detail = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        detail.push_str(" → ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    match user_facing_message(kind) {
        Some(msg) => format!("{msg}\n\nTechnical detail: {detail}"),
        None => format!("API registration failed: {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    #[derive(Debug)]
    struct FakeErr {
        msg: &'static str,
        source: Option<Box<FakeErr>>,
    }
    impl fmt::Display for FakeErr {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.msg)
        }
    }
    impl StdError for FakeErr {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            self.source.as_ref().map(|s| s as &dyn StdError)
        }
    }

    fn chain(msgs: &[&'static str]) -> FakeErr {
        let mut it = msgs.iter().rev();
        let mut err = FakeErr { msg: it.next().unwrap(), source: None };
        for msg in it {
            err = FakeErr { msg, source: Some(Box::new(err)) };
        }
        err
    }

    #[test]
    fn dns_marker_in_chain_classifies_as_dns() {
        let e = chain(&["error sending request", "client error (Connect)", "dns error: failed to lookup address information"]);
        assert_eq!(classify_error_chain(&e), RequestErrorKind::Dns);
    }

    #[test]
    fn getaddrinfo_classifies_as_dns() {
        let e = chain(&["error trying to connect", "getaddrinfo failed"]);
        assert_eq!(classify_error_chain(&e), RequestErrorKind::Dns);
    }

    #[test]
    fn tls_marker_classifies_as_tls() {
        let e = chain(&["error sending request", "invalid peer certificate: Expired"]);
        assert_eq!(classify_error_chain(&e), RequestErrorKind::Tls);
    }

    #[test]
    fn dns_wins_over_tls_when_both_present() {
        let e = chain(&["tls handshake aborted", "dns lookup failed"]);
        assert_eq!(classify_error_chain(&e), RequestErrorKind::Dns);
    }

    #[test]
    fn plain_connect_refusal_is_other_at_chain_level() {
        let e = chain(&["error trying to connect", "tcp connect error: Connection refused (os error 111)"]);
        assert_eq!(classify_error_chain(&e), RequestErrorKind::Other);
    }

    #[test]
    fn dns_message_names_domain_and_dns_servers() {
        let msg = user_facing_message(RequestErrorKind::Dns).unwrap();
        assert!(msg.contains("hardwareapi.frynetworks.com"));
        assert!(msg.contains("1.1.1.1"));
        assert!(msg.contains("8.8.8.8"));
        assert!(msg.contains("carriers"));
    }

    #[test]
    fn connect_and_timeout_and_tls_messages_exist() {
        assert!(user_facing_message(RequestErrorKind::Connect).unwrap().contains("hardwareapi.frynetworks.com"));
        assert!(user_facing_message(RequestErrorKind::Timeout).unwrap().contains("timed out"));
        assert!(user_facing_message(RequestErrorKind::Tls).unwrap().contains("TLS"));
    }

    #[test]
    fn other_kind_has_no_canned_message() {
        assert!(user_facing_message(RequestErrorKind::Other).is_none());
    }

    #[tokio::test]
    async fn real_reqwest_dns_failure_classifies_as_dns() {
        // .invalid is reserved (RFC 2606) — resolution always fails locally.
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let err = client
            .get("https://fem-ub2-test.invalid/health")
            .send()
            .await
            .expect_err("request to .invalid must fail");
        let kind = classify_request_error(&err);
        assert!(
            kind == RequestErrorKind::Dns || kind == RequestErrorKind::Connect,
            "expected Dns (or Connect on unusual resolvers), got {kind:?}"
        );
        let msg = user_facing_registration_error(&err);
        assert!(msg.contains("Technical detail:"));
    }
}
