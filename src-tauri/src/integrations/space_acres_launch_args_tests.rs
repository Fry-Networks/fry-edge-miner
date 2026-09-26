//! c4 BUG LOOP 7 — FEM must launch SpaceAcres with arguments it accepts.
//!
//! start() passed `--base-directory <dir>`. SpaceAcres 0.2.21 has no such
//! option — its own `--help` lists only --startup, --after-crash,
//! --child-process, --uninstall, -h and -V — so every FEM-started instance
//! exited at once on the argument error, the supervisor restarted it into the
//! same error until the budget ran out, and the card said STARTING forever.
//! RC16 evidence (c4-m4-w10-rc16): FEM-started instances died within 30 s,
//! while the same binary started with no arguments stayed up and showed its
//! Welcome window.

/// The options SpaceAcres 0.2.21's `--help` lists that a launcher may pass.
const UPSTREAM_LAUNCH_OPTIONS: [&str; 3] = ["--startup", "--after-crash", "--child-process"];

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn start_passes_only_arguments_space_acres_accepts() {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("async fn start(&self) -> Result<()> {")
        .expect("start must exist");
    let body = &code[at..at + code[at..].find("\n    }\n").expect("start must close")];
    assert!(
        !body.contains(&format!("--base{}", "-directory")),
        "SpaceAcres 0.2.x rejects --base-directory, so every FEM start exits at once:\n{body}"
    );
    assert!(
        body.contains(&format!("cmd.args(launch{}())", "_args")),
        "start() must take its arguments from launch_args():\n{body}"
    );
}

#[test]
fn every_launch_argument_is_one_space_acres_lists() {
    for arg in super::launch_args() {
        assert!(
            UPSTREAM_LAUNCH_OPTIONS.contains(arg),
            "{arg} is not an option SpaceAcres 0.2.21 accepts"
        );
    }
}
