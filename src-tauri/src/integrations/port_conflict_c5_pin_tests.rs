//! c5 F7 pins for lens-1 survivors in the B10 port probe: RC14-PROBE (`&&`
//! -> `||` in `probe`), RC14-REUSE (`#[cfg(unix)]` dropped from the
//! SO_REUSEADDR line) and RC14-TRIP-a/b (`bindable` listening again through
//! an aliased or qualified `TcpListener`, which the existing tripwire's
//! literal `TcpListener::bind(` does not see).
//!
//! REUSE and TRIP change behaviour only on Windows: SO_REUSEADDR there lets
//! a bind share a port a live listener holds, so a held port would read as
//! free; and listening on the wildcard raises the Defender Firewall prompt.
//! Neither is observable on Linux, so they are pinned on the code itself.

/// Comments stripped, so these pins read code and not prose.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every line of port_conflict.rs except its test-module wiring (the tests
/// live in their own files), wherever a `#[cfg(test)]` happens to sit.
fn product_code() -> String {
    code_only(include_str!("port_conflict.rs"))
        .lines()
        .filter(|l| {
            let l = l.trim();
            l != "#[cfg(test)]"
                && !l.starts_with("#[path = ")
                && !(l.starts_with("mod ") && l.ends_with("_tests;"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn bindable_body() -> String {
    let code = product_code();
    let at = code
        .find("fn bindable(addr: (&str, u16)) -> bool {")
        .expect("bindable must exist");
    code[at..at + code[at..].find("\n}").expect("bindable must close")].to_string()
}

/// frynode binds `:<port>` on every interface, so a program listening on any
/// ONE address of that port blocks it. On Linux a listener on 127.0.0.2 makes
/// the wildcard bind fail while 127.0.0.1 stays bindable; only `&&` reports
/// that port as held (RC14-PROBE). Linux only: Windows and the BSDs let a
/// wildcard bind coexist with a specific one.
#[cfg(target_os = "linux")]
#[test]
fn a_holder_on_one_other_address_is_not_free() {
    use super::*;
    let holder =
        std::net::TcpListener::bind("127.0.0.2:0").expect("127.0.0.2 is loopback on Linux");
    let port = holder.local_addr().unwrap().port();
    assert!(
        !bindable(("0.0.0.0", port)),
        "fixture sanity: the holder must block the wildcard bind on port {port}"
    );
    assert!(
        bindable(("127.0.0.1", port)),
        "fixture sanity: 127.0.0.1:{port} must still be bindable"
    );
    assert_ne!(
        probe(port),
        PortState::Free,
        "port {port} is held on 127.0.0.2, so frynode's wildcard bind would fail"
    );
    drop(holder);
}

/// SO_REUSEADDR is set on Unix only; on Windows it would let the probe share
/// a live listener's port (RC14-REUSE).
#[test]
fn reuseaddr_is_set_on_unix_only() {
    let body = bindable_body();
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let at: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains("set_reuseaddr"))
        .collect();
    assert_eq!(at.len(), 1, "exactly one SO_REUSEADDR call:\n{body}");
    assert!(
        at[0] > 0 && lines[at[0] - 1] == "#[cfg(unix)]",
        "the SO_REUSEADDR call must be gated by #[cfg(unix)]:\n{body}"
    );
}

/// No spelling of a listener at all: not `TcpListener` under an alias or a
/// qualified path (RC14-TRIP-a/b), and no `listen(` on a socket.
#[test]
fn the_probe_never_names_a_listener_or_listens() {
    let code = product_code();
    assert!(
        !code.contains("TcpListener"),
        "port_conflict.rs names a TcpListener; listening on the wildcard address raises a \
         Windows Firewall prompt for Fry Edge Miner:\n{code}"
    );
    let listens = regex::Regex::new(r"\blisten\s*\(").unwrap();
    assert!(
        !listens.is_match(&code),
        "port_conflict.rs listens on a socket:\n{code}"
    );
}
