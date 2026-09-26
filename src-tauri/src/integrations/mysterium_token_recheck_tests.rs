//! c4 BUG LOOP 6 — the missing-token state must not expire into a crash.
//!
//! RC15 (BUG LOOP 5) reported a missing Mysterium token for 10 minutes and then
//! fell back to "process is not running" so the supervisor would run start()
//! again. Every such restart spends the supervisor's restart budget and only a
//! Healthy result refunds it, so after ~40 minutes the budget was gone and the
//! card showed UNHEALTHY "Mysterium SDK client process is not running" ~40% of
//! the time (RC15 lens-1 finding, modelled with the shipped supervisor
//! defaults). On expiry the credentials are now re-read by health_check
//! itself: still missing (or unreadable) -> re-arm and keep SETUP REQUIRED;
//! present -> fall through so the supervisor restarts into a real spawn.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn an_expired_window_rechecks_the_credentials_instead_of_reporting_a_crash() {
    let code = code_only(include_str!("mysterium.rs"));
    let at = code
        .find("async fn health_check(&self) -> HealthStatus {")
        .expect("health_check must exist");
    let body = &code[at..];
    // The guard statement itself, so a guard that can never fire does not pass.
    let guard = body
        .find(&format!(
            "let expired = TOKEN_MISSING{}.lock().unwrap().is_some();",
            "_SINCE"
        ))
        .and_then(|g| body[g..].find("if expired {").map(|i| g + i))
        .unwrap_or_else(|| panic!("no live guard around the expiry re-check:\n{body}"));
    let recheck = body[guard..]
        .find(&format!(
            "still_missing_after{}(self.token_present().await)",
            "_recheck"
        ))
        .map(|r| guard + r)
        .unwrap_or_else(|| {
            panic!(
                "an expired missing-token window falls straight through to 'not running':\n{body}"
            )
        });
    let dead = body
        .find(&format!("process_not_running{}(", "_reason"))
        .expect("the generic dead-process reason must remain for real crashes");
    assert!(
        recheck < dead,
        "the re-check must come before the crash reason:\n{body}"
    );
    let branch = &body[recheck..dead];
    assert!(
        branch.contains(&format!(
            "TOKEN_MISSING{} = Some(",
            "_SINCE.lock().unwrap()"
        )) && branch.contains(&format!("TOKEN_NOT_PROVISIONED{}", "_REASON"))
            && branch.contains("return"),
        "a token that is still missing must re-arm the window and keep the setup reason:\n{branch}"
    );
    assert!(
        branch.contains("= None"),
        "a token that has arrived must clear the state so the supervisor restarts into a spawn:\n{branch}"
    );
}

#[test]
fn only_a_token_that_is_now_present_ends_the_setup_state() {
    assert!(!super::still_missing_after_recheck(Some(true)));
    assert!(super::still_missing_after_recheck(Some(false)));
    assert!(
        super::still_missing_after_recheck(None),
        "an unreadable credentials read keeps the last verdict"
    );
}
