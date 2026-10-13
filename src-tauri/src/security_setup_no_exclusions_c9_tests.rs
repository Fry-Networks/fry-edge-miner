//! Spec change (operator-approved): the elevated hardening step no longer
//! writes Microsoft Defender exclusions. The FEM-FryNode firewall part must
//! stay byte-identical. (a)/(b) are RED on the pre-change code, (c) is a
//! guard that must be GREEN both before and after.

use super::{build_hardening_script, manual_hardening_command};
use crate::integrations::firewall;
use std::path::PathBuf;

const BANNED: [&str; 4] = [
    "Add-MpPreference",
    "ExclusionPath",
    "ExclusionProcess",
    "MpPreference",
];

fn fixture() -> (PathBuf, PathBuf) {
    let install_dir = PathBuf::from(r"C:\Users\x\AppData\Local\Fry Edge Miner");
    let frynode = install_dir.join("resources").join("frynode.exe");
    (install_dir, frynode)
}

/// Same quoting the elevated script applies to each reconcile command.
fn expected_firewall_commands(frynode: &std::path::Path) -> Vec<String> {
    firewall::reconcile_commands("FEM-FryNode", &frynode.to_string_lossy())
        .into_iter()
        .map(|argv| {
            let quoted: Vec<String> = argv
                .iter()
                .map(|a| match a.strip_prefix("program=") {
                    Some(p) => format!("program='{}'", p.replace('\'', "''")),
                    None => a.clone(),
                })
                .collect();
            format!("netsh {}", quoted.join(" "))
        })
        .collect()
}

#[test]
fn a_hardening_script_contains_no_defender_exclusion_commands() {
    let (install_dir, frynode) = fixture();
    let script = build_hardening_script(
        &install_dir,
        &["fry-edge-miner.exe", "frynode.exe"],
        &frynode,
    );
    for banned in BANNED {
        assert!(!script.contains(banned), "{banned} found in: {script}");
    }
}

#[test]
fn b_manual_command_has_no_defender_exclusion_and_is_non_empty() {
    let (install_dir, _) = fixture();
    let cmd = manual_hardening_command(&install_dir, &["fry-edge-miner.exe", "frynode.exe"]);
    assert!(!cmd.trim().is_empty(), "manual command must never be empty");
    for banned in BANNED {
        assert!(!cmd.contains(banned), "{banned} found in: {cmd}");
    }
}

#[test]
fn c_firewall_part_of_the_script_is_unchanged() {
    let (install_dir, frynode) = fixture();
    let script = build_hardening_script(
        &install_dir,
        &["fry-edge-miner.exe", "frynode.exe"],
        &frynode,
    );
    let expected = expected_firewall_commands(&frynode);
    assert_eq!(expected.len(), 3);
    for cmd in &expected {
        assert!(script.contains(cmd.as_str()), "missing {cmd} in: {script}");
    }
    assert!(script.contains(&expected.join("; ")));
    assert!(script.contains("netsh advfirewall firewall"));
    assert!(script.contains("FEM-FryNode"));
}
