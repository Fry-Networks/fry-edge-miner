//! c5 F7 pins for R29-P1 / R29-P2 (FAIL-13; #4 left them INCONCLUSIVE).
//! `start()` is Pawns' AUTOMATIC path (boot pass, supervisor restart, Docker
//! watcher). `pawns_no_install_docker_tests.rs` pins that its no-install
//! Docker check is present and comes first, but not that it always runs, and
//! it looks for the installing calls by their literal names only:
//!
//! - R29-P1 nests the check in `if Self::install_marker().exists() { … }`, so
//!   a first start (no marker) skips it and goes straight to `install()` and
//!   the installing `ensure_docker()`.
//! - R29-P2 adds `self.install_for_user()` ahead of it, which satisfies
//!   Docker with `UserClick` authority: a Docker Desktop download and a UAC
//!   prompt that nobody asked for.

const PAWNS_SRC: &str = include_str!("pawns.rs");

/// Comments stripped, so these pins read code and not prose.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `start()`'s body split by brace depth: the text directly in the body
/// (depth 1) and the whole body. Braces inside string literals do not count.
fn start_body() -> (String, String) {
    let code = code_only(PAWNS_SRC);
    let sig = "async fn start(&self) -> Result<()> {";
    let at = code.find(sig).expect("PawnsIntegration::start must exist");
    let body = &code[at + sig.len()..];
    let (mut depth, mut in_str, mut escaped) = (1usize, false, false);
    let mut top = String::new();
    for (i, c) in body.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return (top, body[..i].to_string());
                    }
                }
                _ => {}
            }
        }
        if depth == 1 && c != '}' {
            top.push(c);
        }
    }
    panic!("start() never closes")
}

/// R29-P1: the no-install check is a statement of `start()` itself, not
/// inside any condition, so every automatic start runs it before anything
/// can reach Docker's installing path.
#[test]
fn the_no_install_check_runs_on_every_automatic_start() {
    let (top, body) = start_body();
    assert!(
        top.contains("super::docker_manager::ensure_docker_no_install().await?;"),
        "start() must run the no-install Docker check unconditionally, not inside a condition:\n{body}"
    );
}

/// R29-P2: the automatic path never borrows the user-gesture paths or their
/// authority.
#[test]
fn start_never_takes_the_user_gesture_path() {
    let (_, body) = start_body();
    assert!(
        !body.contains("_for_user("),
        "start() is the automatic path and must not call a *_for_user path:\n{body}"
    );
    assert!(
        !body.contains("UserClick"),
        "start() must not claim a user gesture:\n{body}"
    );
}
