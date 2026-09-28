//! Continuation #5, D10: the scenario guard `require_frynode_port_free` must
//! never LISTEN while it checks frynode's API port. `TcpListener::bind` is
//! bind + listen, and a listen on the wildcard address from the test binary
//! makes Windows Defender Firewall ask the user to allow it — the same dialog
//! c4 BUG LOOP 4 removed from the product's own port probe.
//!
//! The guard's body is Windows-only, so this reads its source: on Linux the
//! body is never compiled, and a behavioural test could not see a listen.

fn guard_body() -> String {
    let src = include_str!("fryvpn_c4_support.rs");
    let start = src
        .find("pub(super) fn require_frynode_port_free()")
        .expect("the scenario port guard still exists");
    let end = src[start..]
        .find("\n}\n")
        .map(|e| start + e)
        .expect("the guard's closing brace");
    // Whitespace removed, so a respelling such as `TcpListener :: bind`
    // cannot slip past the check.
    src[start..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

#[test]
fn the_port_guard_never_listens_while_it_checks_the_port() {
    let body = guard_body();
    assert!(
        !body.contains("TcpListener::bind") && !body.contains("TcpListener::"),
        "D10: the guard binds with TcpListener, which also listens: {body}"
    );
    assert!(
        !body.contains(".listen("),
        "D10: the guard must bind only, never listen: {body}"
    );
}

#[test]
fn the_port_guard_still_checks_the_wildcard_and_the_loopback_address() {
    let body = guard_body();
    assert!(
        body.contains("TcpSocket::new_v4()") && body.contains(".bind("),
        "the guard must still try a bare bind to see whether the port is held: {body}"
    );
    assert!(
        body.contains("(\"0.0.0.0\",FRYNODE_API_PORT)")
            && body.contains("(\"127.0.0.1\",FRYNODE_API_PORT)"),
        "a listener on either address holds frynode's port, so both are checked: {body}"
    );
}
