//! c5 D6 (D-C5-1 knock-on) — a configured SpaceAcres launches with `--startup`;
//! an unconfigured one launches without it, so its setup window is visible.
//!
//! SpaceAcres 0.2.21 (tag commit d4d04b0c4ac3f17ceefd2655becf9bcf8bde54cc)
//! defines the flag in src/main.rs:143-145 ("Used for startup to minimize the
//! window", `#[arg(long)] startup: bool`); src/frontend.rs:765-770 then hides
//! the window (to the tray when there is one), whatever the configuration.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn launch_arguments_follow_the_configuration() {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("pub(crate) fn launch_args() -> &'static [&'static str] {")
        .expect("launch_args must exist");
    let body = &code[at..at + code[at..].find("\n}\n").expect("must close")];
    assert!(
        body.contains("launch_args_for(space_acres_configured())"),
        "the launch arguments must depend on whether SpaceAcres is configured:\n{body}"
    );
}

/// Upstream 0.2.21's own launcher options (src/main.rs:140-160).
const UPSTREAM_LAUNCH_OPTIONS: [&str; 3] = ["--startup", "--after-crash", "--child-process"];

#[test]
fn a_configured_space_acres_starts_minimised_and_an_unconfigured_one_shows_its_setup_window() {
    use super::launch_args_for;
    assert_eq!(launch_args_for(true), ["--startup"]);
    assert!(
        launch_args_for(false).is_empty(),
        "an unconfigured SpaceAcres must start with its setup window visible"
    );
    for arg in launch_args_for(true).iter().chain(launch_args_for(false)) {
        assert!(
            UPSTREAM_LAUNCH_OPTIONS.contains(arg),
            "{arg} is not an option SpaceAcres 0.2.21 accepts"
        );
    }
}
