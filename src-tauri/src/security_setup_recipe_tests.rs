//! A2: hardening re-approval only when the recipe changed or the firewall
//! rules are missing or altered, and the rules are verified on every launch.
//! Pure decision over plain data; no UAC, no firewall, no disk.
use super::{hardening_decision, hardening_recipe_sha, HardeningDecision, HardeningDecision::*};
use crate::integrations::firewall::RuleVerdict::{self, *};
use std::path::Path;

const R: &str = "recipe-a";

fn decide(
    rv: Option<&str>,
    cv: &str,
    rr: Option<&str>,
    cr: &str,
    inb: RuleVerdict,
    out: RuleVerdict,
) -> HardeningDecision {
    hardening_decision(rv, cv, rr, cr, inb, out)
}

#[test]
fn same_recipe_and_intact_rules_on_a_new_version_refresh_the_stamp_silently() {
    assert_eq!(
        decide(Some("0.4.38"), "0.4.39", Some(R), R, Admitted, Admitted),
        RefreshStamp
    );
}

#[test]
fn a_legacy_stamp_without_a_recipe_is_trusted_only_when_both_rules_verify() {
    assert_eq!(
        decide(Some("0.4.38"), "0.4.39", None, R, Admitted, Admitted),
        RefreshStamp
    );
    assert_eq!(
        decide(Some("0.4.38"), "0.4.39", None, R, Admitted, Missing),
        Run
    );
    // Never applied at all: nothing to trust.
    assert_eq!(decide(None, "0.4.39", None, R, Admitted, Admitted), Run);
}

#[test]
fn a_changed_recipe_prompts_even_when_the_rules_look_fine() {
    assert_eq!(
        decide(
            Some("0.4.38"),
            "0.4.39",
            Some("recipe-b"),
            R,
            Admitted,
            Admitted
        ),
        Run
    );
    assert_eq!(
        decide(
            Some("0.4.39"),
            "0.4.39",
            Some("recipe-b"),
            R,
            Admitted,
            Admitted
        ),
        Run
    );
}

#[test]
fn the_same_version_recipe_and_rules_need_nothing() {
    assert_eq!(
        decide(Some("0.4.39"), "0.4.39", Some(R), R, Admitted, Admitted),
        Skip
    );
}

#[test]
fn any_rule_that_is_not_admitted_prompts_in_either_direction_and_at_any_version() {
    let bad = [
        Missing,
        Disabled,
        NotInboundAllow,
        WrongProgram,
        ProfileNotCovered,
        Unreadable,
    ];
    for v in bad {
        for (rv, cv) in [(Some("0.4.38"), "0.4.39"), (Some("0.4.39"), "0.4.39")] {
            assert_eq!(
                decide(rv, cv, Some(R), R, v, Admitted),
                Run,
                "inbound {v:?}"
            );
            assert_eq!(
                decide(rv, cv, Some(R), R, Admitted, v),
                Run,
                "outbound {v:?}"
            );
        }
    }
}

#[test]
fn the_recipe_hash_is_stable_and_follows_the_script_material() {
    let a = hardening_recipe_sha(Path::new(r"C:\x\resources\frynode.exe"));
    assert_eq!(
        a,
        hardening_recipe_sha(Path::new(r"C:\x\resources\frynode.exe"))
    );
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(
        a,
        hardening_recipe_sha(Path::new(r"C:\y\resources\frynode.exe"))
    );
}
