//! c5 D12: `docker_command()` itself cannot pick an installed copy off
//! Windows (`docker_cli_candidates()` is empty elsewhere), so its wiring is
//! read from the source: every resolution goes through `note_resolved_cli`
//! with the one process-wide state, and `docker_command` has no WARN of its
//! own that would fire on each 600 s cache refresh.

/// Comments stripped, so these pins read code and not prose.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn docker_command_body() -> String {
    let code = code_only(include_str!("docker_manager.rs"));
    let at = code
        .find("pub fn docker_command() -> std::process::Command {")
        .expect("docker_command must exist");
    code[at..at + code[at..].find("\n}").expect("docker_command must close")].to_string()
}

#[test]
fn every_resolution_is_announced_through_the_once_per_state_gate() {
    let body = docker_command_body();
    let resolve = body
        .find("pick_docker_cli(on_path, &docker_cli_candidates())")
        .unwrap_or_else(|| panic!("docker_command must still resolve the CLI:\n{body}"));
    assert!(
        body[resolve..].contains(&format!(
            "note_resolved{}(&DOCKER_CLI_LAST_RESOLVED, resolved.as_deref());",
            "_cli"
        )),
        "each resolution must go through the once-per-state gate with the process-wide state:\n{body}"
    );
}

#[test]
fn docker_command_logs_no_warn_of_its_own() {
    let body = docker_command_body();
    assert!(
        !body.contains("warn!("),
        "a WARN directly in docker_command fires on every cache refresh:\n{body}"
    );
}
