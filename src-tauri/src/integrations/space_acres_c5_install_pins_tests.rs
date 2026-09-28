//! c5 pins for two executed survivors in `install_impl` (Q3-CODE-LEVEL-c4).
//!
//! - M10b: the 0a9dbc9^ installer block (msiexec/Burn spawned directly, outside
//!   `run_elevated`) survived because the Automatic precheck's own
//!   `run_elevated(` call satisfied every existing needle.
//! - M28c: `if !precheck_applies(install_trigger)` survived because the
//!   existing needle is a substring of the negated form.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn install_impl_body() -> String {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("async fn install_impl(&self, force: bool) -> Result<()> {")
        .expect("install_impl must exist");
    let end = at + code[at..].find("\n    }\n").expect("must close");
    code[at..end].to_string()
}

#[test]
fn the_installer_spawns_run_inside_the_installers_own_gate_call() {
    let body = install_impl_body();
    let gates: Vec<usize> = body
        .match_indices("elevation_gate::run_elevated(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        gates.len(),
        2,
        "expected the Automatic precheck AND the installer's own gate call:\n{body}"
    );
    let call = &body[gates[1]..];
    let open = call
        .find("move || {")
        .unwrap_or_else(|| panic!("the installer's gate call must take a closure:\n{call}"));
    assert!(
        call[..open].contains("&attempt_key,"),
        "the installer's gate call must track the installer's own attempt key:\n{call}"
    );
    // The closure's own brace span: from its `{` to the matching `}`.
    let start = open + "move || ".len();
    let mut depth = 0usize;
    let mut end = None;
    for (i, c) in call[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.unwrap_or_else(|| panic!("the installer's closure must close:\n{call}"));
    for spawn in ["command(\"msiexec\")", "command(&installer)"] {
        let at = call
            .find(spawn)
            .unwrap_or_else(|| panic!("{spawn} must be spawned by the gate's closure:\n{body}"));
        assert!(
            start < at && at < end,
            "{spawn} runs outside the installer's run_elevated closure:\n{call}"
        );
    }
}

#[test]
fn the_download_precheck_applies_to_automatic_callers_only() {
    let squashed: String = install_impl_body()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        squashed.contains("ifprecheck_applies(install_trigger){"),
        "install_impl must ask the gate up front exactly when precheck_applies says so:\n{squashed}"
    );
    assert!(
        !squashed.contains("!precheck_applies("),
        "a negated precheck sends Automatic callers on to fetch and download:\n{squashed}"
    );
}
