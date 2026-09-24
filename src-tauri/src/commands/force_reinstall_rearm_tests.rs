//! NB-3: the Olostep card's Reinstall is a user gesture and must re-arm the
//! elevation gate the way `toggle_integration` does. The command needs a live
//! `tauri::State<AppState>` (no harness exists: see integration.rs's
//! `bug2_timeout_tests`), so this reads the real source. What the call itself
//! DOES is pinned by
//! `elevation_gate_tests::the_retry_gesture_re_arms_the_one_allowed_attempt`.

const SRC: &str = include_str!("integration.rs");

/// Every step of the gesture that can reach `run_elevated` for this card.
const ELEVATING_STEPS: [&str; 3] = ["force_clean", "install_for_user(", "start_for_user("];

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `pub async fn <name>(` up to its column-0 closing brace.
fn fn_body(code: &str, name: &str) -> String {
    let at = code
        .find(&format!("pub async fn {name}("))
        .unwrap_or_else(|| panic!("{name} must exist"));
    let end = code[at..]
        .find("\n}\n")
        .map(|e| at + e + 2)
        .unwrap_or_else(|| panic!("{name} has no column-0 close"));
    code[at..end].to_string()
}

fn first_elevating_step(body: &str) -> usize {
    ELEVATING_STEPS
        .iter()
        .filter_map(|s| body.find(s))
        .min()
        .expect("no elevation-capable step in this gesture")
}

#[test]
fn force_reinstall_re_arms_the_elevation_gate_before_anything_can_elevate() {
    let code = code_only(SRC);
    let needle = format!("elevation_gate::clear{}(&id)", "_blocked");

    // Positive control: the same predicate over the toggle, which re-arms.
    let toggle = fn_body(&code, &format!("toggle{}", "_integration"));
    let t_at = toggle
        .find(&needle)
        .expect("control: toggle's re-arm not found; needle or slicer is broken");
    assert!(
        t_at < first_elevating_step(&toggle),
        "control: toggle re-arms too late"
    );

    let body = fn_body(&code, &format!("force_reinstall{}", "_integration"));
    assert!(
        body.len() * 4 < code.len(),
        "slice widened: {} of {}",
        body.len(),
        code.len()
    );
    for step in ELEVATING_STEPS {
        assert!(
            body.contains(step),
            "force_reinstall no longer calls {step}; rescope"
        );
    }

    // Past the aem-only guard and the registry lookup: a re-arm placed in
    // either early-return path would sit before force_clean and never run.
    let gesture_starts = body
        .find("Force reinstall: cleaning previous install")
        .expect("control: the gesture's log line is gone; rescope");

    assert!(
        body.contains(&needle),
        "NB-3: force_reinstall_integration never re-arms the elevation gate, so after a \
         declined Olostep firewall prompt Reinstall returns AlreadyAttempted, raises no \
         prompt and republishes the needs-approval message:\n{body}"
    );
    let at = body.find(&needle).unwrap();
    assert!(
        gesture_starts < at && at < first_elevating_step(&body),
        "NB-3: the re-arm must run on the reinstall path itself, before anything can \
         elevate:\n{body}"
    );
}

/// SUPPORTING (green before and after the fix): no automatic path re-arms the gate.
#[test]
fn no_automatic_path_re_arms_the_gate() {
    let main = code_only(include_str!("../main.rs"));
    assert!(
        main.contains(&format!("ElevationTrigger::Auto{}", "matic")),
        "control: main.rs was not really read"
    );
    assert!(
        !main.contains(&format!("clear{}(", "_blocked")),
        "main.rs re-arms the gate on a boot/restart path"
    );
}
