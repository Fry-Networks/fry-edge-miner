//! SpaceAcres elevation gate (row 10): `install_impl`'s msiexec/Burn spawn
//! (:706-724 at 5b7c8f5) ran OUTSIDE `elevation_gate` entirely — an automatic
//! path (boot recovery, an automatic reinstall) could put a real install (and
//! whatever consent prompt msiexec/Burn's own manifest raises) on screen with
//! nobody at the keyboard.
//!
//! `install_impl`'s signature is pinned to a single `force: bool` parameter
//! by `commands::integration_update_lock_tests::space_acres_update_forces_the_reinstall_and_restarts`,
//! which text-matches `self.install_impl(true)` verbatim inside
//! `apply_update` — so it cannot take an `ElevationTrigger` parameter.
//! `next_install_is_user_gesture` (an `AtomicBool` field, read-and-reset as
//! the FIRST thing `install_impl` does, before its first `.await`) is the
//! seam that lets `install_for_user`/`apply_update` (both reachable only from
//! a real user gesture — see their own doc comments in space_acres.rs) hand
//! off `UserClick` without changing that signature. `install()` (the boot
//! pass / health-loop path) never sets it, so `install_impl` reads `false`
//! (Automatic) by default.
//!
//! The actual msiexec/Burn spawn is `#[cfg(target_os = "windows")]`-only, so
//! — exactly as this file's own `b21_blocking_offload_tests` already does for
//! the blocking-offload property one item up — these read the real source
//! rather than executing it; the binding runtime proof is a VM run.

const SPACE_ACRES_SRC: &str = include_str!("space_acres.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract exactly ONE function's body (from its signature's opening `{` to
/// its OWN matching closing `}`, by brace balance) — safe against the
/// end-of-file / sibling-distance failure mode a "scan to the next `fn`"
/// bound has (see `pawns_no_install_docker_tests.rs`'s `fn_body`, the same
/// helper, duplicated here since each `#[path]` test file in this crate is
/// self-contained).
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

/// THE fix: `install_impl`'s installer spawn is routed through
/// `elevation_gate::run_elevated`, under the "space_acres" purpose.
#[test]
fn install_impls_installer_spawn_goes_through_the_elevation_gate() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(
        &code,
        "async fn install_impl(&self, force: bool) -> Result<()> {",
    );

    assert!(
        body.contains("elevation_gate::run_elevated("),
        "install_impl's installer spawn must go through the elevation gate: {body}"
    );
    assert!(
        body.contains("\"space_acres\""),
        "the elevation must be scoped to the space_acres purpose: {body}"
    );
    // Non-vacuity: the two literal `output_bounded` calls (msiexec, Burn)
    // must be inside the `run_elevated` closure, not before it — otherwise
    // the gate call is present but does not actually wrap the spawn.
    let gate_at = body.find("elevation_gate::run_elevated(").unwrap();
    let bounded_positions: Vec<usize> = body
        .match_indices("output_bounded(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        bounded_positions.len(),
        2,
        "expected msiexec + Burn: {body}"
    );
    assert!(
        bounded_positions.iter().all(|&at| at > gate_at),
        "both output_bounded calls must be AFTER (inside) the run_elevated call, \
         not before it: {body}"
    );
}

/// The trigger `install_impl` uses must be read (and reset) before the
/// function's first `.await` — the whole hand-off relies on there being no
/// yield point between a caller setting the flag and `install_impl` reading
/// it.
#[test]
fn install_impl_reads_the_trigger_flag_before_its_first_await() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(
        &code,
        "async fn install_impl(&self, force: bool) -> Result<()> {",
    );

    let read_at = body
        .find("next_install_is_user_gesture")
        .expect("install_impl must read the trigger flag: {body}");
    let first_await = body
        .find(".await")
        .expect("install_impl must still await something: {body}");
    assert!(
        read_at < first_await,
        "the flag must be read before install_impl's first .await, or a \
         concurrent caller could reset it first: {body}"
    );
}

/// The user-initiated toggle path must exist and set the flag BEFORE calling
/// `install_impl`.
#[test]
fn install_for_user_sets_the_flag_before_calling_install_impl() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(&code, "async fn install_for_user(&self) -> Result<()> {");

    let set_at = body
        .find("next_install_is_user_gesture")
        .unwrap_or_else(|| panic!("install_for_user must set the trigger flag: {body}"));
    let call_at = body
        .find("self.install_impl(")
        .unwrap_or_else(|| panic!("install_for_user must call install_impl: {body}"));
    assert!(
        set_at < call_at,
        "the flag must be set BEFORE calling install_impl: {body}"
    );
    assert!(body.contains("store(true"), "must set it to true: {body}");
}

/// `apply_update` — reached ONLY from `install_update`, a `#[tauri::command]`
/// (the Updates page's "Update" click), never automatically — must ALSO set
/// the flag before its (frozen-text) `self.install_impl(true)` call.
#[test]
fn apply_update_sets_the_flag_before_reinstalling() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(
        &code,
        "async fn apply_update(&self, version: &str) -> Result<()> {",
    );

    let set_at = body
        .find("next_install_is_user_gesture")
        .unwrap_or_else(|| panic!("apply_update must set the trigger flag: {body}"));
    let call_at = body.find("self.install_impl(true)").unwrap_or_else(|| {
        panic!("FAIL-5's own frozen requirement: apply_update must force the reinstall: {body}")
    });
    assert!(
        set_at < call_at,
        "the flag must be set BEFORE the reinstall: {body}"
    );
}

/// `install()` — the boot pass / health-loop path — must NEVER set the flag,
/// or an automatic caller could accidentally borrow user authority.
#[test]
fn install_never_sets_the_user_gesture_flag() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(&code, "async fn install(&self) -> Result<()> {");
    assert!(
        !body.contains("next_install_is_user_gesture"),
        "install() must not touch the trigger flag — it is the automatic \
         path and must always get Automatic: {body}"
    );
}

/// Fixture sanity: `fn_body` really does stop at the function's own end.
#[test]
fn fn_body_does_not_run_past_the_functions_own_end() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(&code, "async fn install_for_user(&self) -> Result<()> {");
    assert!(
        !body.contains("mod ") && !body.contains("async fn start"),
        "install_for_user's extracted body ran past its own end: {body}"
    );
}

// ---------------------------------------------------------------------
// BUG LOOP 2, item 6(a) (NB): the installer_path-derived attempt_key never
// changes between versions or attempts, and install_for_user/apply_update
// never re-armed it — so a second UserClick install in the same FEM run
// (e.g. toggle-on, then later an Update click) was refused as
// AlreadyAttempted, exactly the B3 "retry gesture was a guaranteed no-op"
// shape clear_blocked exists to prevent.
// ---------------------------------------------------------------------

#[test]
fn install_for_user_rearms_the_gate_before_installing() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(&code, "async fn install_for_user(&self) -> Result<()> {");
    let clear_at = body
        .find("clear_blocked(\"space_acres\")")
        .unwrap_or_else(|| panic!("install_for_user must re-arm the gate: {body}"));
    let call_at = body
        .find("self.install_impl(")
        .unwrap_or_else(|| panic!("install_for_user must still call install_impl: {body}"));
    assert!(
        clear_at < call_at,
        "clear_blocked(\"space_acres\") must run BEFORE install_impl, or this \
         gesture's own attempt could be cleared out from under it: {body}"
    );
}

#[test]
fn apply_update_rearms_the_gate_before_reinstalling() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(
        &code,
        "async fn apply_update(&self, version: &str) -> Result<()> {",
    );
    let clear_at = body
        .find("clear_blocked(\"space_acres\")")
        .unwrap_or_else(|| panic!("apply_update must re-arm the gate: {body}"));
    let call_at = body.find("self.install_impl(true)").unwrap_or_else(|| {
        panic!("FAIL-5's own frozen requirement: apply_update must force the reinstall: {body}")
    });
    assert!(
        clear_at < call_at,
        "clear_blocked(\"space_acres\") must run BEFORE the reinstall: {body}"
    );
}

// ---------------------------------------------------------------------
// BUG LOOP 2, item 6(b) (NB): the existing tests check only that
// `next_install_is_user_gesture` is read before the first `.await`, and that
// `store(true` appears in install_for_user/apply_update — neither ties
// run_elevated's ACTUAL trigger argument to the flag's value, so an inverted
// mapping (swap(false, ..) == true -> Automatic) or a hardcoded
// `ElevationTrigger::UserClick` literal passed to run_elevated survives
// every existing test.
// ---------------------------------------------------------------------

#[test]
fn the_swap_true_branch_maps_to_user_click_and_false_to_automatic() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(
        &code,
        "async fn install_impl(&self, force: bool) -> Result<()> {",
    );

    let swap_at = body
        .find(".swap(false,")
        .unwrap_or_else(|| panic!("must read-and-reset the flag via swap(false, ..): {body}"));
    let after_swap = &body[swap_at..];
    let user_click_at = after_swap
        .find("ElevationTrigger::UserClick")
        .unwrap_or_else(|| panic!("UserClick must appear after the swap: {body}"));
    let automatic_at = after_swap
        .find("ElevationTrigger::Automatic")
        .unwrap_or_else(|| panic!("Automatic must appear after the swap: {body}"));
    assert!(
        user_click_at < automatic_at,
        "the swap's TRUE branch (the flag WAS set — a real user gesture) \
         must map to UserClick and therefore appear textually first (the \
         if-branch), before Automatic (the else-branch) — an inverted \
         mapping would give the boot pass UserClick authority: {body}"
    );
}

#[test]
fn every_run_elevated_call_passes_the_computed_install_trigger_not_a_literal() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(
        &code,
        "async fn install_impl(&self, force: bool) -> Result<()> {",
    );

    let mut cursor = 0usize;
    let mut checked = 0usize;
    while let Some(rel) = body[cursor..].find("elevation_gate::run_elevated(") {
        let at = cursor + rel;
        cursor = at + "elevation_gate::run_elevated(".len();
        let window = &body[at..(at + 300).min(body.len())];
        assert!(
            window.contains("install_trigger,"),
            "every run_elevated call in install_impl must pass the computed \
             install_trigger, not a hardcoded ElevationTrigger literal — a \
             literal UserClick here would give an automatic caller user \
             authority: {window}"
        );
        checked += 1;
    }
    assert!(
        checked >= 1,
        "control: must find at least one run_elevated call in install_impl: {body}"
    );
}

// ---------------------------------------------------------------------
// BUG LOOP 2, item 6(c) (NB): install_impl fetched the release and
// downloaded the FULL installer before ever asking the gate — and Automatic
// is ALWAYS refused, so an automatic path (boot recovery with SpaceAcres
// enabled but not installed) downloaded on every single launch only to have
// the gate refuse it every time.
// ---------------------------------------------------------------------

#[test]
fn automatic_never_downloads_before_the_gate_refuses_it() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(
        &code,
        "async fn install_impl(&self, force: bool) -> Result<()> {",
    );

    let precheck_at = body
        .find("ElevationTrigger::Automatic {")
        .unwrap_or_else(|| {
            panic!("install_impl must check install_trigger == Automatic before fetching/downloading: {body}")
        });
    let fetch_at = body.find("fetch_latest_release()").unwrap_or_else(|| {
        panic!("install_impl must still fetch the release when allowed: {body}")
    });
    let download_at = body
        .find("download_file_with_options(")
        .unwrap_or_else(|| panic!("install_impl must still download when allowed: {body}"));

    assert!(
        precheck_at < fetch_at,
        "the Automatic gate pre-check must run BEFORE fetch_latest_release — \
         an automatic path must never call the GitHub API just to have the \
         gate refuse the install: {body}"
    );
    assert!(
        precheck_at < download_at,
        "the Automatic gate pre-check must also run BEFORE the download: {body}"
    );
}

/// The dropped-`?` mutant class (BUG LOOP 2 item 5): the pre-check's
/// refusal must actually propagate, not just be present in the source.
#[test]
fn the_automatic_precheck_propagates_its_refusal() {
    let code = code_only(SPACE_ACRES_SRC);
    let body = fn_body(
        &code,
        "async fn install_impl(&self, force: bool) -> Result<()> {",
    );

    let precheck_at = body
        .find("ElevationTrigger::Automatic {")
        .expect("must exist — proven by the sibling test above");
    let window = &body[precheck_at..(precheck_at + 400).min(body.len())];
    assert!(
        window.contains("run_elevated(") && window.contains(")?;"),
        "the Automatic pre-check must propagate a refusal with `?`, not \
         silently discard it: {window}"
    );
}
