//! c5 pins for MUT's surviving mutants in space_acres.rs (E5/mutants/1-*.txt),
//! and for the same mutations applied to the D13 owned-image kill path that
//! replaced BL8's kill_tree block (RC18-K1..K3 and K6 are moot on that code).
//!
//! The kill and sweep code is Windows-only, so on Linux these are source pins
//! over comment-free code, like the BL8 pins in space_acres_tree_stop_tests.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn body(signature: &str, close: &str) -> String {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} must exist"));
    let end = at + code[at..].find(close).expect("must close");
    code[at..end].to_string()
}

fn squashed(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// RC17-ARG / RC17-FMT: an argument added in start() itself bypasses
/// launch_args() and the upstream option check.
#[test]
fn start_passes_arguments_only_through_launch_args() {
    let start = body("async fn start(&self) -> Result<()> {", "\n    }\n");
    assert_eq!(
        start.matches(".arg(").count(),
        0,
        "start() adds an argument of its own:\n{start}"
    );
    assert_eq!(
        start.matches(".args(").count(),
        1,
        "start() must take every argument from launch_args() alone:\n{start}"
    );
    assert!(start.contains("cmd.args(launch_args());"), "{start}");
}

/// RC18-K4 / RC18-K5: the adoption sweep force-stops both images, on Windows.
#[test]
fn the_adoption_sweep_force_stops_both_images_on_windows() {
    let stop = body("async fn stop(&self) -> Result<()> {", "\n    }\n");
    let windows = stop
        .find("#[cfg(target_os = \"windows\")]")
        .unwrap_or_else(|| panic!("stop() must keep its Windows sweep:\n{stop}"));
    let other = stop
        .find("#[cfg(not(target_os = \"windows\"))]")
        .unwrap_or_else(|| panic!("stop() must keep its non-Windows sweep:\n{stop}"));
    assert!(windows < other, "{stop}");
    for sweep in [
        ".args([\"/IM\", \"space-acres.exe\", \"/F\"])",
        ".args([\"/IM\", \"space-acres-modern.exe\", \"/F\"])",
    ] {
        let at = stop
            .find(sweep)
            .unwrap_or_else(|| panic!("the sweep must force-stop by image: {sweep}\n{stop}"));
        assert!(
            windows < at && at < other,
            "{sweep} must run in stop()'s Windows block:\n{stop}"
        );
    }
}

/// D13 analogues of RC18-K1..K3: the owned-image kill really spawns taskkill
/// with the owned arguments and waits for it, and the listing really runs.
#[test]
fn the_owned_kill_spawns_taskkill_on_the_listed_pids_and_waits() {
    let kill = squashed(&body("fn kill_tree(pid: u32) -> bool {", "\n}\n"));
    assert!(
        kill.contains(
            "command(\"taskkill\").args(owned_kill_args(&pids)).output_bounded(crate::supervisor::platform::PROBE_TIMEOUT)"
        ),
        "kill_tree must spawn taskkill with the owned pids and wait for it:\n{kill}"
    );
    assert!(
        kill.contains(
            "command(\"powershell\").args([\"-NoProfile\",\"-Command\",query.as_str()]).output_bounded(crate::supervisor::platform::PROBE_TIMEOUT).ok().filter(|o|o.status.success())"
        ),
        "kill_tree must run the listing and accept only a successful one:\n{kill}"
    );
    assert!(
        kill.contains("rows.is_some()}#[cfg(not(target_os=\"windows\"))]"),
        "on Windows kill_tree must report whether the listing ran:\n{kill}"
    );
}

/// D13/D4 analogue of RC18-K6: the tracked tree kill is unconditional, in
/// stop() and at exit.
#[test]
fn the_tracked_tree_kill_is_unconditional() {
    let stop = squashed(&body("async fn stop(&self) -> Result<()> {", "\n    }\n"));
    assert!(
        stop.contains("lettree_known=kill_tree(child.id());let_=child.kill();"),
        "stop() must kill the tracked tree every time:\n{stop}"
    );
    let exit = squashed(&body(
        "async fn stop_for_exit(&self) -> Result<()> {",
        "\n    }\n",
    ));
    assert!(
        exit.contains("{kill_tree(child.id());let_=child.kill();"),
        "stop_for_exit() must kill the FEM-spawned tree every time:\n{exit}"
    );
}
