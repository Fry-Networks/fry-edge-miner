//! FAIL-1 (pre-tag review of 0.4.34): a Docker integration's install elevates
//! under Docker Desktop's own gate purpose, not the card's id, and the enable
//! gesture re-armed only the card's id. One Docker Desktop attempt therefore
//! refused every later Docker enable (Diiisco, Filecoin, Pawns, Sentinel)
//! until FEM restarted, after re-downloading the installer each time.
//!
//! `toggle_integration` needs a live `tauri::State<AppState>`, so this reads the
//! real source. What the helper does is proven behaviourally by
//! `integration::fail1_docker_rearm_tests`.

const INTEGRATION: &str = include_str!("integration.rs");
const DOCKER: &str = include_str!("../integrations/docker_manager.rs");

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

/// The purpose `run_docker_installer` elevates under, read from the real file.
fn docker_desktop_purpose() -> String {
    let code = code_only(DOCKER);
    let at = code
        .find("elevation_gate::run_elevated(\"")
        .expect("control: the Docker installer must go through the gate");
    let rest = &code[at + "elevation_gate::run_elevated(\"".len()..];
    rest[..rest.find('"').expect("purpose literal must close")].to_string()
}

#[test]
fn a_docker_enable_re_arms_the_purpose_docker_desktop_elevates_under() {
    let code = code_only(INTEGRATION);
    let toggle = fn_body(&code, &format!("toggle{}", "_integration"));

    // Positive control: the card's own re-arm, before anything can install.
    let card = toggle
        .find("elevation_gate::clear_blocked(&id)")
        .expect("control: the toggle's card re-arm is gone; rescope");
    let install = toggle
        .find("install_for_user(")
        .expect("control: the toggle no longer installs; rescope");
    assert!(card < install, "control: the card re-arm runs too late");

    let call = format!(
        "rearm_docker{}(integration.requires_docker())",
        "_prerequisite"
    );
    assert!(
        toggle.contains(&call),
        "FAIL-1: the enable gesture never re-arms Docker Desktop's elevation purpose, so \
         one Docker install attempt refuses every later Docker enable until restart:\n{toggle}"
    );
    let at = toggle.find(&call).unwrap();
    assert!(
        card < at && at < install,
        "FAIL-1: the Docker re-arm must run with the card's, before the install:\n{toggle}"
    );

    let purpose = docker_desktop_purpose();
    assert_ne!(purpose, "", "control: empty purpose literal");
    let helper_at = code
        .find(&format!("fn rearm_docker{}(", "_prerequisite"))
        .expect("FAIL-1: the re-arm helper must exist");
    let helper = &code[helper_at..helper_at + code[helper_at..].find("\n}\n").unwrap()];
    assert!(
        helper.contains(&format!("clear_blocked(\"{purpose}\")")),
        "FAIL-1: the helper must clear exactly the purpose the Docker installer elevates \
         under ({purpose}):\n{helper}"
    );
}
