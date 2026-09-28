//! c5 F7 pin for lens-1 survivor RC14-X7: a Docker-absent refusal that is
//! present but can never fire. `docker_gesture_download_tests.rs` and
//! `docker_refusal_text_c5_tests.rs` read a guard's block only up to its
//! first `}`, so `if !may_fetch_docker_installer(trigger) { if false {
//! anyhow::bail!(…); } }` passed both, and an Automatic caller fell through
//! to the ~600 MB installer download (FAIL-13). Each guard's whole block,
//! braces balanced, must be the refusal and nothing else.

const REFUSAL: &str = r#"anyhow::bail!("Install Docker Desktop, then turn this on again.");"#;

/// Comments stripped, so these guards read code and not prose.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text of `ensure_docker_core`'s `NotInstalled` arm up to the download.
fn not_installed_arm() -> String {
    let code = code_only(include_str!("docker_manager.rs"));
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

/// Everything between the guard's `{` and its MATCHING `}`, whitespace
/// collapsed. Braces inside string literals do not count.
fn whole_block(arm: &str, stmt: &str) -> String {
    let at = arm
        .find(stmt)
        .unwrap_or_else(|| panic!("the NotInstalled arm must still refuse with `{stmt}`:\n{arm}"));
    let body = &arm[at + stmt.len()..];
    let (mut depth, mut in_str, mut escaped) = (1usize, false, false);
    for (i, c) in body.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return body[..i].split_whitespace().collect::<Vec<_>>().join(" ");
                }
            }
            _ => {}
        }
    }
    panic!("the guard block never closes:\n{body}")
}

#[test]
fn the_trigger_guard_does_nothing_but_refuse() {
    let arm = not_installed_arm();
    let stmt = format!("if !may_fetch{}(trigger) {{", "_docker_installer");
    assert_eq!(
        whole_block(&arm, &stmt),
        REFUSAL,
        "an Automatic caller must be refused unconditionally, before the download:\n{arm}"
    );
}

#[test]
fn the_allow_install_guard_does_nothing_but_refuse() {
    let arm = not_installed_arm();
    assert_eq!(
        whole_block(&arm, "if !allow_install {"),
        REFUSAL,
        "a caller that may not install must be refused unconditionally:\n{arm}"
    );
}
