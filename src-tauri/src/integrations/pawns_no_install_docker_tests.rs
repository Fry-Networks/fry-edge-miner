//! FAIL-13 (row 7): `PawnsIntegration::start()` is reached by the
//! supervisor's automatic restart path (`stop_for_restart` -> `start()`), the
//! boot pass, and the Docker watcher — never a user gesture. It called
//! `ensure_docker()`, whose `NotInstalled` branch downloads the Docker
//! Desktop installer BEFORE the elevation gate is ever consulted (only the
//! install/elevate step was gated, not the download). A restart with Docker
//! absent therefore put a real, gesture-less network fetch on the wire.
//!
//! These read the real sources (the same technique
//! `docker_boot_contract_tests.rs` already uses in this crate), because
//! actually driving Docker through a real download/install in a unit test is
//! neither safe nor portable — and on a box with no reachable Docker daemon
//! (this one), even `wait_for_docker`'s bounded polling loop would make the
//! test slow and environment-dependent.
//!
//! COMPLIANCE (Pawns CLI Addendum §5.2-5.4): FAIL-13's fix may only ADD a
//! user-gesture requirement. Nothing in `start()`'s existing consent gate
//! (`user_consent()`, `consent_required_status()`, `credentials()`) is
//! touched by this fix, and none of the consent test files reference
//! `start()`/`ensure_docker` at all (grepped: they test the consent
//! predicates in isolation), so this item cannot have disturbed them.

const PAWNS_SRC: &str = include_str!("pawns.rs");
const DOCKER_SRC: &str = include_str!("docker_manager.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract exactly ONE function's body (from its signature's opening `{` to
/// its OWN matching closing `}`, by brace balance).
///
/// `docker_boot_contract_tests.rs` bounds a scan at "the next `pub async fn`"
/// — which only terminates correctly when a same-visibility sibling follows
/// soon after. Two of the functions this file scans
/// (`ensure_docker_no_install`, `ensure_docker_core`) are the LAST function of
/// their kind in `docker_manager.rs`: that pattern would run to end-of-file,
/// across every inline test module below them — hundreds of lines that could
/// vacuously satisfy a `.contains(...)` check regardless of what the scanned
/// function actually does. Brace balance has no such failure mode.
fn fn_body<'a>(code: &'a str, signature_needle: &str) -> &'a str {
    let at = code
        .find(signature_needle)
        .unwrap_or_else(|| panic!("signature not found: {signature_needle}"));
    let open = at
        + signature_needle
            .rfind('{')
            .expect("signature needle must end at the opening brace");
    let bytes = code.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &code[at..=i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("no matching closing brace for: {signature_needle}");
}

/// THE fix: the automatic path (`start()`) calls the no-install check, never
/// the installing `ensure_docker()`.
#[test]
fn start_never_calls_the_installing_ensure_docker() {
    let code = code_only(PAWNS_SRC);
    let body = fn_body(&code, "async fn start(&self) -> Result<()> {");

    assert!(
        body.contains("ensure_docker_no_install()"),
        "start() must call the no-install docker check: {body}"
    );
    assert!(
        !body.contains("ensure_docker()"),
        "start() — the automatic path — must not call the installing \
         ensure_docker(), which downloads the Docker Desktop installer \
         before the elevation gate is ever consulted: {body}"
    );
}

/// The user-initiated path must still exist, or Docker can never be
/// installed for Pawns at all (mirrors `install_for_user`, already pinned by
/// the frozen `docker_boot_contract_tests::every_docker_integration_has_a_user_initiated_install_path`
/// for the INSTALL side; this is the same requirement for START).
#[test]
fn start_for_user_satisfies_docker_with_user_click_before_delegating_to_start() {
    let code = code_only(PAWNS_SRC);
    let body = fn_body(&code, "async fn start_for_user(&self) -> Result<()> {");
    assert!(
        body.contains("ElevationTrigger::UserClick"),
        "start_for_user must carry user authority: {body}"
    );
    assert!(
        body.contains("ensure_docker_with"),
        "start_for_user must be able to install: {body}"
    );
}

/// `ensure_docker_core`: when installing is not allowed, the guard must be
/// checked BEFORE the download call is ever reached — not merely present
/// somewhere in the same function.
#[test]
fn ensure_docker_core_bails_before_downloading_when_install_is_not_allowed() {
    let code = code_only(DOCKER_SRC);
    let body = fn_body(
        &code,
        "async fn ensure_docker_core(\n    trigger: crate::elevation_gate::ElevationTrigger,\n    allow_install: bool,\n) -> Result<()> {",
    );

    let guard_at = body
        .find("if !allow_install")
        .unwrap_or_else(|| panic!("must check allow_install before installing: {body}"));
    let download_at = body
        .find("download_docker_installer")
        .unwrap_or_else(|| panic!("must still download when allowed: {body}"));
    assert!(
        guard_at < download_at,
        "the allow_install guard must come BEFORE the download call, or a caller \
         with allow_install=false could still reach it: {body}"
    );
}

/// `ensure_docker_no_install`'s own body must pass `false` for
/// `allow_install` — the whole guarantee rests on this wiring.
#[test]
fn ensure_docker_no_install_passes_false_for_allow_install() {
    let code = code_only(DOCKER_SRC);
    let body = fn_body(
        &code,
        "pub async fn ensure_docker_no_install() -> Result<()> {",
    );
    assert!(
        body.contains("ensure_docker_core(") && body.contains(", false)"),
        "ensure_docker_no_install must call ensure_docker_core(.., false): {body}"
    );
}

/// Fixture sanity: `fn_body` really does stop at the right place, not at
/// end-of-file — proves the tests above are not accidentally scanning past
/// `ensure_docker_no_install`/`ensure_docker_core` into unrelated code (the
/// exact failure mode this module's doc comment warns about).
#[test]
fn fn_body_does_not_run_past_the_functions_own_end() {
    let code = code_only(DOCKER_SRC);
    let body = fn_body(
        &code,
        "pub async fn ensure_docker_no_install() -> Result<()> {",
    );
    assert!(
        !body.contains("mod "),
        "ensure_docker_no_install's extracted body must not contain a `mod` \
         keyword — if it does, fn_body ran past the function into a test \
         module below it: {body}"
    );
    assert!(
        body.len() < 200,
        "expected a short one-line body, got {} bytes",
        body.len()
    );
}

// ---------------------------------------------------------------------
// Chunk-3 fix B: start_for_user must run the SAME preconditions start()
// runs, in the SAME order, BEFORE ensuring Docker. It used to call
// ensure_docker_with (a real Docker Desktop download + UAC prompt on a
// fresh install) before ever checking consent or credentials — a user who
// toggled Pawns on WITHOUT consent got a download/UAC prompt, and only
// THEN "needs your consent".
// ---------------------------------------------------------------------

/// THE fix: the consent check precedes ensure_docker_with.
#[test]
fn start_for_user_checks_consent_before_ensuring_docker() {
    let code = code_only(PAWNS_SRC);
    let body = fn_body(&code, "async fn start_for_user(&self) -> Result<()> {");

    let consent_at = body
        .find("user_consent()")
        .unwrap_or_else(|| panic!("start_for_user must check user_consent(): {body}"));
    let docker_at = body
        .find("ensure_docker_with(")
        .unwrap_or_else(|| panic!("start_for_user must still ensure Docker: {body}"));
    assert!(
        consent_at < docker_at,
        "the consent check must run BEFORE ensure_docker_with — otherwise a \
         user without consent gets a Docker Desktop download/UAC prompt \
         before ever being told they need to consent: {body}"
    );
}

/// The credentials check must ALSO precede Docker — same reasoning, same
/// precondition start() itself checks second.
#[test]
fn start_for_user_checks_credentials_before_ensuring_docker() {
    let code = code_only(PAWNS_SRC);
    let body = fn_body(&code, "async fn start_for_user(&self) -> Result<()> {");

    let creds_at = body
        .find("self.credentials()")
        .unwrap_or_else(|| panic!("start_for_user must check credentials(): {body}"));
    let docker_at = body
        .find("ensure_docker_with(")
        .unwrap_or_else(|| panic!("start_for_user must still ensure Docker: {body}"));
    assert!(
        creds_at < docker_at,
        "the credentials check must run BEFORE ensure_docker_with: {body}"
    );
}

/// The reused message, not a new one — a caller-visible behavioural
/// guarantee: this must be `consent_required_status()`, the exact function
/// `start()` and the consent-required card already use.
#[test]
fn start_for_user_reuses_the_exact_consent_message_function() {
    let code = code_only(PAWNS_SRC);
    let body = fn_body(&code, "async fn start_for_user(&self) -> Result<()> {");
    assert!(
        body.contains("consent_required_status()"),
        "must reuse the exact consent message start() uses, not a new one: {body}"
    );
}

// A behavioural version of the same property (drive start_for_user() for
// real with no consent, assert Err(consent_required_status()) fast) was
// deliberately NOT added: user_consent() is `consent_from_env_value
// (PAWNS_USER_CONSENT) || Self::consent_active()`, and PAWNS_USER_CONSENT /
// PAWNS_DEVICE_ID are process-global env vars also READ by every consent
// test in pawns_consent_tests.rs / pawns_restart_consent_tests.rs /
// pawns_anchored_consent_tests.rs, which run concurrently in the same test
// binary and which this item must not touch or add synchronization to. The
// three structural tests above prove the exact same ordering property
// deterministically, with no shared-state race.
