//! B10 — port :8088 conflicts.

use super::*;
use crate::integrations::HealthStatus;
use crate::supervisor::health::{recovery_action, RecoveryAction};

/// The reason a held port produces must PAUSE recovery, not drive it: nothing
/// FEM restarts can take a port away from another program, and doing it every
/// 30 s is how the restart budget was burned before the user ever read the
/// card.
#[test]
fn a_port_conflict_reason_pauses_recovery_instead_of_restarting() {
    let reason = conflict_reason(
        &PortState::HeldBy {
            pid: 4312,
            image: "python.exe".to_string(),
        },
        8088,
    );

    assert!(
        crate::integrations::awaits_user_action(&reason),
        "must be recognised as waiting on something FEM cannot fix: {reason}"
    );
    assert_eq!(
        recovery_action(&HealthStatus::Unhealthy(reason), true, 0, 6),
        RecoveryAction::None
    );
}

/// §6 B10 Done-when: "the card names the conflicting process".
#[test]
fn the_reason_names_the_process_and_the_port() {
    let reason = conflict_reason(
        &PortState::HeldBy {
            pid: 4312,
            image: "python.exe".to_string(),
        },
        8088,
    );
    assert!(reason.contains("8088"), "{reason}");
    assert!(reason.contains("python.exe"), "{reason}");
    assert!(reason.contains("4312"), "{reason}");
}

/// An unresolvable owner still has to produce a usable sentence: a socket held
/// by another user or a higher integrity level is not always visible to FEM's
/// non-elevated session.
#[test]
fn an_unresolved_owner_still_produces_a_pausing_reason() {
    let reason = conflict_reason(&PortState::Held, 8088);
    assert!(reason.contains("8088"), "{reason}");
    assert!(crate::integrations::awaits_user_action(&reason));
}

#[test]
fn a_bound_port_never_probes_as_free() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    assert_ne!(
        probe(port),
        PortState::Free,
        "a port with a live listener must never be reported bindable"
    );

    drop(listener);
}

#[test]
fn a_released_port_probes_as_free() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    assert_eq!(
        probe(port),
        PortState::Free,
        "once the listener is gone the probe must stop blaming it"
    );
}

#[test]
fn the_listening_pid_is_read_from_real_netstat_output() {
    let listing = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1084
  TCP    0.0.0.0:8088           0.0.0.0:0              LISTENING       4312
  TCP    192.168.1.22:52144     140.82.113.26:443      ESTABLISHED     9001
  TCP    [::]:8088              [::]:0                 LISTENING       4312
";
    assert_eq!(listening_pid(listing, 8088), Some(4312));
    assert_eq!(listening_pid(listing, 135), Some(1084));
    assert_eq!(listening_pid(listing, 9999), None);
}

/// An ESTABLISHED connection whose LOCAL ephemeral port happens to equal the
/// one we care about is not holding it.
#[test]
fn an_established_connection_is_not_mistaken_for_a_listener() {
    let listing = "\
  Proto  Local Address          Foreign Address        State           PID
  TCP    192.168.1.22:8088      140.82.113.26:443      ESTABLISHED     9001
";
    assert_eq!(listening_pid(listing, 8088), None);
}

#[test]
fn the_image_name_is_read_from_real_tasklist_output() {
    let out = "\
python.exe                    4312 Console                    1     12,345 K
";
    assert_eq!(first_image(out), Some("python.exe".to_string()));
}

/// Image names really do contain spaces.
#[test]
fn an_image_name_with_spaces_survives() {
    let out = "\
Fry Edge Miner.exe            7788 Console                    1     98,765 K
";
    assert_eq!(first_image(out), Some("Fry Edge Miner.exe".to_string()));
}

#[test]
fn a_tasklist_miss_is_not_an_image() {
    assert_eq!(
        first_image("INFO: No tasks are running which match the specified criteria."),
        None
    );
    assert_eq!(first_image(""), None);
}
