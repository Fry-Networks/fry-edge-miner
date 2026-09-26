//! c4 BUG LOOP 4 (BL4-B) — the B10 port probe must not LISTEN on the wildcard.
//!
//! `probe()` tested bindability with `std::net::TcpListener::bind`, which is
//! bind + listen. Listening on 0.0.0.0 from fry-edge-miner.exe makes Windows
//! Defender Firewall raise its "Do you want to allow public and private
//! networks to access this app?" dialog for Fry Edge Miner and create
//! Query-User rules — a prompt with no user gesture, new in 0.4.34. RC13
//! evidence (c4-b3-uac, W11): FEM's auto-created rules removed, a 75 s window
//! with no trigger produced none; killing frynode (health_check's not-running
//! branch -> this probe) re-created them within 4 s. A bind alone answers
//! "is the port free" exactly as well, without asking the firewall anything.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// port_conflict.rs without its trailing test wiring.
fn product_code() -> String {
    let code = code_only(include_str!("port_conflict.rs"));
    let end = code.find("#[cfg(test)]").unwrap_or(code.len());
    code[..end].to_string()
}

#[test]
fn the_probe_never_listens_to_test_a_port() {
    let code = product_code();
    assert!(
        !code.contains(&format!("TcpListener::{}(", "bind")),
        "port_conflict.rs listens to test a port; listening on the wildcard address raises a \
         Windows Firewall prompt for Fry Edge Miner:\n{code}"
    );
}

#[test]
fn the_probe_still_tests_both_addresses() {
    let code = product_code();
    let at = code
        .find("pub(crate) fn probe(port: u16) -> PortState {")
        .expect("probe must exist");
    let body = &code[at..at + code[at..].find("\n}").expect("probe must close")];
    assert!(
        body.contains("bindable((\"0.0.0.0\", port))")
            && body.contains("bindable((\"127.0.0.1\", port))"),
        "frynode binds all interfaces, so a holder on either address blocks it:\n{body}"
    );
}
