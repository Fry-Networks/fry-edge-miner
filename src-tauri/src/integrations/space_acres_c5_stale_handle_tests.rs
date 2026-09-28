//! c5 D4 — a stale tracked handle must not leave SpaceAcres' farmer running.
//!
//! stop() took the tracked handle and killed its tree. When the supervisor had
//! already exited, there was no tree left to walk: its farmer
//! (`space-acres-modern.exe`) ran on as an orphan, and stop() returned before
//! the image sweep that would have caught it.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn stop_checks_for_a_stale_handle_before_killing_and_can_fall_through() {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("async fn stop(&self) -> Result<()> {")
        .expect("stop must exist");
    let body = &code[at..at + code[at..].find("\n    }\n").expect("must close")];
    let kill = body
        .find("kill_tree(child.id())")
        .unwrap_or_else(|| panic!("stop() must still stop the tracked tree:\n{body}"));
    let probe = body.find("try_wait()").unwrap_or_else(|| {
        panic!("stop() never asks whether the tracked supervisor already exited:\n{body}")
    });
    assert!(
        probe < kill,
        "the exit check must come before the kill, while it still means something:\n{body}"
    );
    let ret = kill
        + body[kill..]
            .find("return Ok(());")
            .expect("the tracked branch returns");
    assert!(
        body[kill..ret].contains("if "),
        "the tracked branch returns unconditionally, so a stale handle skips the sweep:\n{body}"
    );
}

#[test]
fn stop_decides_on_the_exit_it_saw_and_on_the_listing() {
    let code = code_only(include_str!("space_acres.rs"));
    assert!(
        code.contains("let exited = matches!(child.try_wait(), Ok(Some(_)));")
            && code.contains("if tracked_stop_is_complete(exited, tree_known) {"),
        "stop() must decide the sweep from the tracked child's exit and the listing"
    );
}

#[test]
fn only_a_live_supervisor_with_a_listed_tree_skips_the_sweep() {
    use super::tracked_stop_is_complete;
    assert!(tracked_stop_is_complete(false, true));
    assert!(
        !tracked_stop_is_complete(true, true),
        "a stale handle must fall through to the sweep"
    );
    assert!(!tracked_stop_is_complete(false, false));
    assert!(!tracked_stop_is_complete(true, false));
}
