//! A2: the outbound rule hardening installs is verified like the inbound one.
use super::{outbound_rule_verdict, rule_verdict, RuleVerdict};

const PROG: &str = r"C:\x\resources\frynode.exe";

fn listing(rules: &[&str]) -> String {
    let mut s = String::new();
    for r in rules {
        s.push_str(&format!("RULE|{r}|{PROG}\n"));
    }
    s.push_str("NET|Private\nEND\n");
    s
}

#[test]
fn an_outbound_allow_rule_is_admitted_and_judged_separately_from_the_inbound_one() {
    let both = listing(&["Inbound|True|Allow|Any", "Outbound|True|Allow|Any"]);
    assert_eq!(
        outbound_rule_verdict(Some(&both), PROG),
        RuleVerdict::Admitted
    );
    assert_eq!(rule_verdict(Some(&both), PROG), RuleVerdict::Admitted);

    let inbound_only = listing(&["Inbound|True|Allow|Any"]);
    assert_eq!(
        rule_verdict(Some(&inbound_only), PROG),
        RuleVerdict::Admitted
    );
    assert_ne!(
        outbound_rule_verdict(Some(&inbound_only), PROG),
        RuleVerdict::Admitted
    );
}

#[test]
fn a_disabled_or_wrong_program_outbound_rule_is_not_admitted_and_a_cut_listing_fails_closed() {
    let off = listing(&["Inbound|True|Allow|Any", "Outbound|False|Allow|Any"]);
    assert_eq!(
        outbound_rule_verdict(Some(&off), PROG),
        RuleVerdict::Disabled
    );
    let other = "RULE|Outbound|True|Allow|Any|C:\\other.exe\nNET|Private\nEND\n";
    assert_eq!(
        outbound_rule_verdict(Some(other), PROG),
        RuleVerdict::WrongProgram
    );
    assert_eq!(outbound_rule_verdict(None, PROG), RuleVerdict::Unreadable);
    assert_eq!(
        outbound_rule_verdict(Some("RULE|Outbound|True|Allow|Any|x"), PROG),
        RuleVerdict::Unreadable
    );
}
