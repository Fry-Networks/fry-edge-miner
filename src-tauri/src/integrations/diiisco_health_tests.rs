//! RC38 / D9: Diiisco reported Healthy whenever something answered 2xx on
//! :8181, with no node container running. Health now also needs the
//! `diiisco-node` container to be running. The docker and HTTP seams are
//! faked (no Docker, no network).
use super::{health_gate, http_health, node_container_ps_args, HealthStatus, HttpProbe};
use reqwest::StatusCode;

/// The verdict health_check reaches: the gate first, the HTTP half only
/// when the gate lets the node through.
fn classify_health(node_running: Option<bool>, http: HttpProbe) -> HealthStatus {
    match health_gate(node_running) {
        Err(status) => status,
        Ok(()) => http_health(http),
    }
}

fn ok(code: u16) -> HttpProbe {
    HttpProbe::Status(StatusCode::from_u16(code).unwrap())
}

#[test]
fn no_node_container_is_stopped_even_when_something_answers_200() {
    assert_eq!(classify_health(Some(false), ok(200)), HealthStatus::Stopped);
    assert_eq!(classify_health(Some(false), ok(204)), HealthStatus::Stopped);
}

#[test]
fn a_running_node_answering_2xx_is_healthy() {
    assert_eq!(classify_health(Some(true), ok(200)), HealthStatus::Healthy);
    assert_eq!(classify_health(Some(true), ok(204)), HealthStatus::Healthy);
}

#[test]
fn a_running_node_that_refuses_connections_is_stopped() {
    assert_eq!(
        classify_health(Some(true), HttpProbe::Refused),
        HealthStatus::Stopped
    );
}

#[test]
fn a_running_node_with_a_bad_status_or_a_timeout_is_unhealthy() {
    assert_eq!(
        classify_health(Some(true), ok(500)),
        HealthStatus::Unhealthy("HTTP 500 Internal Server Error".into())
    );
    assert_eq!(
        classify_health(Some(true), HttpProbe::Timeout),
        HealthStatus::Unhealthy("Timeout".into())
    );
}

#[test]
fn a_failed_container_check_is_unhealthy_never_healthy() {
    assert_eq!(
        classify_health(None, ok(200)),
        HealthStatus::Unhealthy("cannot verify node container".into())
    );
}

#[test]
fn the_gate_only_lets_a_running_node_through_to_the_http_probe() {
    assert_eq!(health_gate(Some(true)), Ok(()));
    assert_eq!(health_gate(Some(false)), Err(HealthStatus::Stopped));
    assert!(matches!(health_gate(None), Err(HealthStatus::Unhealthy(_))));
    // the verdict of the http half alone is unchanged
    assert_eq!(http_health(ok(200)), HealthStatus::Healthy);
}

#[test]
fn the_container_check_asks_docker_for_the_running_container_by_exact_name() {
    assert_eq!(
        node_container_ps_args(),
        vec![
            "ps",
            "--filter",
            "name=^/?diiisco-node$",
            "--filter",
            "status=running",
            "-q"
        ]
    );
}

#[test]
fn health_check_asks_for_the_container_before_it_sends_the_http_request() {
    let src = include_str!("diiisco.rs");
    let body = src.split("#[cfg(test)]").next().unwrap();
    let hc = body.split("async fn health_check(&self)").nth(1).unwrap();
    let hc = hc.split("async fn check_update(").next().unwrap();
    let at = |n: &str| hc.find(n).unwrap_or(usize::MAX);
    assert!(at("docker_available()") < at("node_container_running()"));
    assert!(
        at("health_gate(") < at("client.get(HEALTH_URL)"),
        "no HTTP when the node is not there"
    );
    assert!(at("node_container_running()") < at("health_gate("));
}

// ------------------------------------------- the `docker ps -q` verdict

use super::parse_node_running;

#[test]
fn an_id_line_means_the_node_container_is_running() {
    assert_eq!(parse_node_running(true, b"3f9a1c0d2b7e\n"), Some(true));
    assert_eq!(parse_node_running(true, b"3f9a1c0d2b7e"), Some(true));
}

#[test]
fn empty_or_blank_output_means_not_running() {
    assert_eq!(parse_node_running(true, b""), Some(false));
    assert_eq!(parse_node_running(true, b"\n  \n\t\n"), Some(false));
}

#[test]
fn a_failed_docker_call_is_unknown_whatever_it_printed() {
    assert_eq!(parse_node_running(false, b""), None);
    assert_eq!(parse_node_running(false, b"3f9a1c0d2b7e\n"), None);
    // and unknown becomes Unhealthy, never Healthy
    assert!(matches!(
        health_gate(parse_node_running(false, b"")),
        Err(HealthStatus::Unhealthy(m)) if m == "cannot verify node container"
    ));
}
