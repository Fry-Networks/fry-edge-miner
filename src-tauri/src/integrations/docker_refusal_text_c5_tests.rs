//! c5 D7 (D-C5-11): with Docker Desktop absent, the refusal FEM puts on the
//! card must tell the user what to do. The old text sent them to "its
//! Settings action", which does not exist. Both refusals in
//! `ensure_docker_core`'s `NotInstalled` arm carry the one ruled text: the
//! `allow_install` refusal (`ensure_docker_no_install`, Pawns' supervisor
//! restart) and the trigger refusal (every Automatic caller of
//! `ensure_docker`, which is what turning an already-installed Sentinel,
//! Diiisco or Filecoin Checker card on reaches through `start()`).

const REFUSAL: &str = "Install Docker Desktop, then turn this on again.";

const DOCKER_SRC: &str = include_str!("docker_manager.rs");

/// Comments stripped, so these guards read code and not prose.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text of `ensure_docker_core`'s `NotInstalled` arm up to the download.
fn not_installed_arm() -> String {
    let code = code_only(DOCKER_SRC);
    let core = code
        .find(&format!("async fn ensure_docker{}(", "_core"))
        .expect("ensure_docker_core must exist");
    let arm = code[core..]
        .find("DockerStatus::NotInstalled =>")
        .map(|a| core + a)
        .expect("ensure_docker_core must still handle NotInstalled");
    let download = code[arm..]
        .find(&format!("download_docker{}()", "_installer"))
        .map(|d| arm + d)
        .expect("a user gesture must still be able to download the installer");
    code[arm..download].to_string()
}

/// The block the guard statement `stmt` opens, up to its closing brace.
fn guard_block(arm: &str, stmt: &str) -> String {
    let at = arm
        .find(stmt)
        .unwrap_or_else(|| panic!("the NotInstalled arm must still refuse with `{stmt}`:\n{arm}"));
    let block = &arm[at + stmt.len()..];
    block[..block.find('}').expect("the guard block must close")].to_string()
}

/// The message the block's `bail!` returns: its one string literal, with
/// Rust's `\` line continuations resolved the way the compiler resolves them.
fn bail_text(block: &str) -> String {
    let at = block
        .find("bail!(")
        .unwrap_or_else(|| panic!("the guard must refuse with bail!:\n{block}"));
    let args = &block[at + "bail!(".len()..];
    let args = args.trim_start();
    assert!(
        args.starts_with('"'),
        "the refusal must be one plain string literal, so the card shows it as written:\n{block}"
    );
    let body = &args[1..];
    let end = body
        .char_indices()
        .find(|&(i, c)| c == '"' && !body[..i].ends_with('\\'))
        .map(|(i, _)| i)
        .expect("the refusal literal must close");
    let rest = body[end + 1..].trim_start();
    assert!(
        rest.starts_with(')'),
        "the refusal must carry no format arguments:\n{block}"
    );
    let mut out = String::new();
    let mut lines = body[..end].split('\n');
    out.push_str(lines.next().unwrap_or(""));
    for line in lines {
        assert!(
            out.ends_with('\\'),
            "a raw newline inside the refusal:\n{block}"
        );
        out.pop();
        out.push_str(line.trim_start());
    }
    out
}

#[test]
fn the_allow_install_refusal_is_the_ruled_text() {
    let arm = not_installed_arm();
    assert_eq!(
        bail_text(&guard_block(&arm, "if !allow_install {")),
        REFUSAL
    );
}

#[test]
fn the_trigger_refusal_is_the_ruled_text() {
    let arm = not_installed_arm();
    let stmt = format!("if !may_fetch{}(trigger) {{", "_docker_installer");
    assert_eq!(bail_text(&guard_block(&arm, &stmt)), REFUSAL);
}

/// Neither refusal may keep pointing at a Settings action FEM does not have.
#[test]
fn no_refusal_names_a_settings_action() {
    let arm = not_installed_arm();
    assert!(
        !arm.contains("Settings action"),
        "the NotInstalled arm still names a Settings action:\n{arm}"
    );
}
