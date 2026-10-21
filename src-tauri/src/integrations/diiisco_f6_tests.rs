//! F6: a `diiisco-node` container in Docker's `restarting` state is not
//! running, so health reads Stopped. This was already true implicitly (the ps
//! filter is `status=running`); these pins make it explicit. Docker's filter
//! semantics are taken from the code and its argv, not from a live daemon.
use super::{health_gate, node_container_ps_args, parse_node_running, HealthStatus};

#[test]
fn the_ps_filter_asks_for_running_containers_only_so_a_restarting_one_never_matches() {
    let args = node_container_ps_args();
    assert!(args.contains(&"status=running"), "{args:?}");
    assert!(
        !args
            .iter()
            .any(|a| a.contains("restarting") || *a == "-a" || *a == "--all"),
        "{args:?}"
    );
}

#[test]
fn a_restarting_container_lists_no_id_and_reads_as_stopped() {
    // `docker ps -q --filter status=running` prints nothing for it.
    assert_eq!(parse_node_running(true, b""), Some(false));
    assert_eq!(
        health_gate(parse_node_running(true, b"\n")),
        Err(HealthStatus::Stopped)
    );
    // A running one prints its id.
    assert_eq!(parse_node_running(true, b"abc123\n"), Some(true));
}
