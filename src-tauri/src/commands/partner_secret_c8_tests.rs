//! D-C8-1: the save path writes `<target>.tmp.<pid>` and renames it; a kill
//! between the two (or a rename AND a remove that both fail) left that temp,
//! which holds the whole config including the Iagon node token, forever.
//!
//! - Every save sweeps stale temps next to the target before it writes.
//! - Iagon's token reader sweeps once per process, before anything else.

/// `src` with every `//` comment cut off, so prose cannot satisfy a guard.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The function that writes the temp must sweep before it writes it.
#[test]
fn the_save_path_sweeps_stale_temps_before_writing_its_own() {
    let code = code_only(include_str!("partner_secret.rs"));
    let write = code
        .find("std::fs::write(&tmp")
        .expect("the save path still writes a temp");
    let enclosing = code[..write].rfind("fn ").expect("enclosing fn");
    assert!(
        code[enclosing..write].contains("sweep_stale_secret_temps("),
        "the save path must call sweep_stale_secret_temps( before std::fs::write(&tmp"
    );
}

/// The only load path for this secret is Iagon's `node_token()`; it must sweep
/// first, even when the token comes from the environment.
#[test]
fn iagon_node_token_sweeps_before_the_env_var_early_return() {
    let code = code_only(include_str!("../integrations/iagon.rs"));
    let start = code
        .find("fn node_token()")
        .expect("iagon.rs still has fn node_token()");
    let env = start
        + code[start..]
            .find("IAGON_NODE_TOKEN")
            .expect("node_token still reads IAGON_NODE_TOKEN");
    assert!(
        code[start..env].contains("sweep_stale_secret_temps("),
        "node_token() must call sweep_stale_secret_temps( before IAGON_NODE_TOKEN"
    );
}
