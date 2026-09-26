//! c4 BUG LOOP 5 — MystNodes' card must say that the partner token is missing.
//!
//! `start()` refuses to spawn sdk_client when the device credentials carry no
//! `mystnodes_user_token` (fail closed, no user-claimable fallback), but
//! `health_check()` only ever saw "no process" and reported
//! "Mysterium SDK client process is not running" — UNHEALTHY, as if it had
//! crashed. RC14 evidence (c4-m4-w11): the card said exactly that while fem.log
//! said 'mystnodes_user_token not found or empty in device credentials —
//! contact support'. M4 requires RUNNING or an ACCURATE setup-required state;
//! a wrong-but-tidy state is a fail.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn body_of(code: &str, signature: &str) -> String {
    let at = code
        .find(signature)
        .unwrap_or_else(|| panic!("{signature} must exist"));
    let end = code[at..]
        .find("\n    }\n")
        .map(|e| at + e)
        .unwrap_or(code.len());
    code[at..end].to_string()
}

/// The dead-process branch reports the missing token before it falls back to
/// the generic "not running" reason.
#[test]
fn a_missing_token_is_what_the_health_check_reports() {
    let code = code_only(include_str!("mysterium.rs"));
    let body = body_of(&code, "async fn health_check(&self) -> HealthStatus {");
    // The guard statement itself, so a guard that can never fire
    // (`if false && …`) does not pass.
    let token = body
        .find(&format!("if token_missing{}(", "_recently"))
        .unwrap_or_else(|| {
            panic!(
                "health_check never looks at the missing-token state, so a device without a \
                 Mysterium token shows 'process is not running' instead of what to do:\n{body}"
            )
        });
    let dead = body
        .find(&format!("process_not_running{}(", "_reason"))
        .expect("the generic dead-process reason must remain for real crashes");
    assert!(
        token < dead,
        "the missing-token reason must win over the generic dead-process reason:\n{body}"
    );
    let branch = &body[token..dead];
    assert!(
        branch.contains(&format!("TOKEN_NOT_PROVISIONED{}", "_REASON"))
            && branch.contains("return"),
        "the missing-token branch must return the provisioning reason:\n{branch}"
    );
}

/// start() records the refusal and a successful spawn clears it.
#[test]
fn start_records_a_missing_token_and_a_spawn_clears_it() {
    let code = code_only(include_str!("mysterium.rs"));
    let body = body_of(&code, "async fn start(&self) -> Result<()> {");
    let set = body
        .find(&format!(
            "TOKEN_MISSING{} = Some(",
            "_SINCE.lock().unwrap()"
        ))
        .or_else(|| body.find(&format!("*TOKEN_MISSING{}", "_SINCE")))
        .unwrap_or_else(|| panic!("start() does not record the missing token:\n{body}"));
    let spawn = body
        .find("start_integration(\"mysterium\"")
        .expect("start() must still spawn sdk_client");
    assert!(
        set < spawn,
        "the refusal is recorded before (instead of) the spawn:\n{body}"
    );
    assert!(
        body[spawn..].contains("= None"),
        "a successful spawn must clear the missing-token state:\n{body}"
    );
}

#[test]
fn the_reason_names_the_token_and_pauses_recovery() {
    use crate::integrations::HealthStatus;
    use crate::supervisor::health::{recovery_action, RecoveryAction};
    let reason = super::TOKEN_NOT_PROVISIONED_REASON;
    assert!(
        reason.contains("Mysterium") && reason.contains("support"),
        "{reason}"
    );
    assert!(crate::integrations::awaits_user_action(reason), "{reason}");
    assert_eq!(
        recovery_action(&HealthStatus::Unhealthy(reason.to_string()), true, 0, 6),
        RecoveryAction::None,
        "a missing token is not a crash to restart into"
    );
}

#[test]
fn the_missing_token_state_expires_so_a_provisioned_token_is_picked_up() {
    use std::time::{Duration, Instant};
    let now = Instant::now();
    assert!(!super::token_missing_recently(None, now));
    assert!(super::token_missing_recently(
        Some(now - Duration::from_secs(60)),
        now
    ));
    assert!(!super::token_missing_recently(
        Some(now - Duration::from_secs(601)),
        now
    ));
}
