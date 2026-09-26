//! c4 BUG LOOP 5 — lens-1 RC14 NB: `ensure_docker_with` must hand its
//! caller's trigger to the core. A literal UserClick there would reopen both
//! the gesture-less download and the gesture-less UAC prompt, and no earlier
//! test read this function.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn ensure_docker_with_passes_its_callers_trigger() {
    let code = code_only(include_str!("docker_manager.rs"));
    let at = code
        .find(&format!("pub async fn ensure_docker{}(trigger", "_with"))
        .expect("ensure_docker_with must exist");
    let body = &code[at..at + code[at..].find("\n}").expect("must close")];
    assert!(
        body.contains("ensure_docker_core(trigger, true)"),
        "ensure_docker_with must pass the caller's own trigger through:\n{body}"
    );
    assert!(
        !body.contains(&format!("ElevationTrigger::User{}", "Click")),
        "ensure_docker_with must not claim a gesture of its own:\n{body}"
    );
}
