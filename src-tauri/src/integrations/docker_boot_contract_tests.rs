//! B3 / G4 finding 22 — the boot contract, enforced in code rather than
//! restated in a doc comment.
//!
//! `ensure_docker`'s comment says "only call on an explicit user action — never
//! at app boot". That held only as a property of two call sites happening to
//! check `requires_docker()`/`docker_status()` first, not as a property of the
//! function. It is now structural: `ensure_docker()` passes
//! `ElevationTrigger::Automatic`, which the gate refuses before any prompt
//! appears, and only `install_for_user` — reached from the toggle and from
//! force-reinstall — passes `UserClick`.
//!
//! The review also found the test that was SUPPOSED to pin this vacuous: it
//! called `run_elevated` with its own closure and a counter, touching no line
//! of docker_manager.rs, so it stayed green whatever the installer did. These
//! read the real sources.

/// Needles assembled at runtime so these guards cannot match their own text.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

const DOCKER_SRC: &str = include_str!("docker_manager.rs");

/// The Docker-backed integrations. Each reaches the elevated installer through
/// `ensure_docker`, so each must keep the automatic paths on Automatic.
const DOCKER_INTEGRATIONS: [(&str, &str); 4] = [
    ("diiisco.rs", include_str!("diiisco.rs")),
    ("sentinel.rs", include_str!("sentinel.rs")),
    ("pawns.rs", include_str!("pawns.rs")),
    ("filecoin_checker.rs", include_str!("filecoin_checker.rs")),
];

/// The default entry point must never be able to raise a prompt.
#[test]
fn the_default_ensure_docker_uses_an_automatic_trigger() {
    let code = code_only(DOCKER_SRC);
    let at = code
        .find(&format!(
            "pub async fn ensure{}() -> Result<()> {{",
            "_docker"
        ))
        .expect("ensure_docker must exist");
    let end = code[at..]
        .find("\npub async fn ")
        .map(|e| at + e)
        .unwrap_or(code.len());
    let body = &code[at..end];

    assert!(
        body.contains(&format!("ElevationTrigger::Auto{}", "matic")),
        "ensure_docker() can raise a UAC prompt on a path nobody asked for:\n{body}"
    );
    assert!(
        !body.contains(&format!("ElevationTrigger::User{}", "Click")),
        "ensure_docker() claims a user gesture it cannot know about:\n{body}"
    );
}

/// The elevated installer must take its caller's authority, not assume one.
#[test]
fn the_docker_installer_takes_the_callers_trigger() {
    let code = code_only(DOCKER_SRC);
    let at = code
        .find(&format!("async fn run_docker{}(", "_installer"))
        .expect("run_docker_installer must exist");
    let end = code[at..]
        .find("\nasync fn ")
        .map(|e| at + e)
        .unwrap_or(code.len());
    let body = &code[at..end];

    assert!(
        body.contains("trigger: crate::elevation_gate::ElevationTrigger"),
        "the installer does not accept a trigger, so it elevates on its own authority:\n{body}"
    );
    assert!(
        body.contains(&format!("elevation_gate::run{}(", "_elevated")),
        "the installer does not go through the elevation gate:\n{body}"
    );
}

/// No `start()` may claim user authority. A user gesture reaches Docker
/// through `install_for_user`; `start()` is also called by the supervisor's
/// restart path, the boot pass and the Docker watcher.
#[test]
fn no_start_impl_asks_docker_for_a_user_click() {
    let user = format!("ElevationTrigger::User{}", "Click");

    for (name, src) in DOCKER_INTEGRATIONS {
        let code = code_only(src);
        let Some(at) = code.find(&format!("async fn start(&{}) -> Result<()> {{", "self")) else {
            continue;
        };
        let end = code[at..]
            .find("\n    async fn ")
            .map(|e| at + e)
            .unwrap_or(code.len());
        let body = &code[at..end];

        assert!(
            !body.contains(&user),
            "{name}: start() claims a user gesture, but the supervisor, the boot pass and the \
             Docker watcher all call start() with nobody at the keyboard:\n{body}"
        );
    }
}

/// And the user-initiated path must actually exist on each of them, or the
/// ratified toggle-off/on retry is a dead end: every attempt would be refused.
#[test]
fn every_docker_integration_has_a_user_initiated_install_path() {
    let user = format!("ElevationTrigger::User{}", "Click");
    let hook = format!("async fn install_for{}(&self)", "_user");

    for (name, src) in DOCKER_INTEGRATIONS {
        let code = code_only(src);
        assert!(
            code.contains(&hook),
            "{name} has no user-initiated install, so a declined Docker prompt can never be retried"
        );
        assert!(
            code.contains(&user),
            "{name}'s user-initiated install does not carry user authority, so the gate refuses \
             it forever"
        );
    }
}
