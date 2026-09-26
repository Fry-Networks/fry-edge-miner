//! c4 BUG LOOP 8 — turning SpaceAcres off must stop its farmer, not only its
//! supervisor.
//!
//! A bare `space-acres.exe` launch is upstream's supervisor; the GUI/farmer
//! runs as a separate `space-acres-modern.exe --child-process` (the upstream
//! `--help` names the flag). stop() killed only the tracked supervisor, so on
//! RC17 (c4-m4-w10, toggle OFF 05:03:55Z) FEM logged "Stopped SpaceAcres
//! (tracked child)" while space-acres-modern.exe kept running and farming —
//! the card said Off. The tracked tree is now stopped as a whole, and the
//! adoption sweep also covers the farmer's image.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn body_of(code: &str, signature: &str) -> String {
    let at = code
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} must exist"));
    let end = at + code[at..].find("\n    }\n").expect("must close");
    code[at..end].to_string()
}

fn tree_kill_precedes_the_supervisor_kill(body: &str, what: &str) {
    let tree = body
        .find(&format!("kill_{}(child.id())", "tree"))
        .unwrap_or_else(|| {
            panic!("{what} kills only the supervisor; the farmer survives:\n{body}")
        });
    let kill = body
        .find("child.kill()")
        .unwrap_or_else(|| panic!("{what} must still kill the tracked child:\n{body}"));
    assert!(
        tree < kill,
        "{what}: the tree must be stopped while its parent link still exists:\n{body}"
    );
}

#[test]
fn stop_takes_the_whole_tracked_tree_down() {
    let code = code_only(include_str!("space_acres.rs"));
    tree_kill_precedes_the_supervisor_kill(
        &body_of(&code, "async fn stop(&self) -> Result<()> {"),
        "stop()",
    );
}

#[test]
fn exit_stops_the_whole_tree_fem_spawned() {
    let code = code_only(include_str!("space_acres.rs"));
    tree_kill_precedes_the_supervisor_kill(
        &body_of(&code, "async fn stop_for_exit(&self) -> Result<()> {"),
        "stop_for_exit()",
    );
}

#[test]
fn the_adoption_sweep_also_stops_the_farmer_image() {
    let code = code_only(include_str!("space_acres.rs"));
    let body = body_of(&code, "async fn stop(&self) -> Result<()> {");
    assert!(
        body.contains(&format!("\"space-acres-{}.exe\"", "modern")),
        "the image sweep leaves space-acres-modern.exe (the farmer) running:\n{body}"
    );
}

#[test]
fn the_tree_kill_targets_the_pid_and_its_descendants_by_force() {
    assert_eq!(super::kill_tree_args(4321), ["/PID", "4321", "/T", "/F"]);
}
