//! BUG LOOP 2 — the heartbeat notice follows the node's state.
//!
//! `get_integrations` put the notice on the card whatever the node was doing:
//! on a dead frynode it replaced the real Unhealthy reason, because the card's
//! error line suppresses the reason line. And a refresh whose wallet read was
//! still in flight when the user disabled fryDVPN wrote the notice back into
//! the freshly reset state, so the disabled card said "fryDVPN is running".

#[cfg(unix)]
use super::fryvpn_c4_support::*;
#[cfg(unix)]
use super::*;

/// The notice belongs only to a node that is enabled AND Healthy. Pinned on
/// the source because `get_integrations` needs a live Tauri app.
#[test]
fn the_card_shows_the_notice_only_for_an_enabled_healthy_fryvpn() {
    let src = include_str!("../commands/integration.rs");
    let healthy = src
        .find("let healthy = matches!(health, HealthStatus::Healthy);")
        .expect("get_integrations computes `healthy` from the health it reports");
    let start = src[healthy..]
        .find("error: last_errors")
        .map(|i| healthy + i);
    assert!(start.is_some(), "the error line is built after `healthy`");
    let expr = &src[start.unwrap()..];
    let open = expr
        .find(".or_else(|| {")
        .expect("the notice merge is an .or_else closure");
    let close = expr[open..]
        .find(".then(crate::integrations::fryvpn::funding_notice)")
        .map(|i| open + i);
    assert!(close.is_some(), "the notice is still merged:\n{expr}");
    let condition: String = expr[open + ".or_else(|| {".len()..close.unwrap()]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(
        condition, r#"(id=="fryvpn"&&enabled&&healthy)"#,
        "the notice must be gated on fryDVPN, enabled AND Healthy — a dead or disabled node's \
         card shows its real state"
    );
}

#[cfg(unix)]
#[test]
fn a_disable_clears_the_notice_and_an_in_flight_read_cannot_bring_it_back() {
    run_scenario_on_frynode_port(module_path!(), "disable_clears_the_notice");
}

#[cfg(unix)]
#[test]
#[ignore = "scenario entry point: the tests above run it in a child process"]
fn child() {
    let Ok(scenario) = std::env::var(SCENARIO_VAR) else {
        return;
    };
    match scenario.as_str() {
        "disable_clears_the_notice" => disable_clears_the_notice(),
        other => panic!("unknown scenario {other}"),
    }
    println!("{DONE} {scenario}");
}

/// A second handle on the scene's integration (same supervisor, same state),
/// so a health check can run on the runtime while the test disables it.
#[cfg(unix)]
fn twin(s: &Scene) -> Arc<FryVpnIntegration> {
    Arc::new(FryVpnIntegration {
        config: s.integ.config.clone(),
        api_client: s.integ.api_client.clone(),
        supervisor: s.integ.supervisor.clone(),
        log_dir: s.integ.log_dir.clone(),
    })
}

#[cfg(unix)]
fn disable_clears_the_notice() {
    const ENTERED: &str = "cannot pay its next heartbeat";
    let s = Scene::new(
        World::new(100_014, RegistryBox::Present),
        Endpoint::PortInline,
        None,
    );
    install_decoy_frynode(&s);
    let health = frynode_health_decoy();

    // A registered node short of one heartbeat fee: the notice is up.
    let started = s.block_on(s.integ.start());
    assert!(started.is_ok(), "{started:?}");
    assert!(funding_notice().is_some(), "fixture: the notice is set");

    // A user disable clears it.
    let _ = s.block_on(s.integ.stop_for_disable());
    assert_eq!(funding_notice(), None, "a disable drops the notice");

    // Enabled again, funded, notice clear; then drained, and the health
    // check's re-read is slow. The user disables while it is in flight.
    s.set_world(|w| w.amount = 400_000);
    let restarted = s.block_on(s.integ.start());
    assert!(restarted.is_ok(), "{restarted:?}");
    assert_eq!(funding_notice(), None, "fixture: funded, no notice");
    s.set_world(|w| {
        w.amount = 100_014;
        w.account_delay = Duration::from_secs(2);
    });
    let entered_before = s.log_lines(ENTERED).len();
    let check = {
        let integ = twin(&s);
        s.rt.spawn(async move { integ.health_check().await })
    };
    // The check is due (first after the start) and now waits on the read.
    std::thread::sleep(Duration::from_millis(600));
    let _ = s.block_on(s.integ.stop_for_disable());
    let _ = s.block_on(check);
    assert_eq!(
        funding_notice(),
        None,
        "a read that finished after the disable must not put the notice back"
    );
    assert_eq!(
        s.log_lines(ENTERED).len(),
        entered_before,
        "nor log a shortfall for a node that is no longer running"
    );
    assert!(
        s.reads(&account_path()) >= 3,
        "fixture: the in-flight read really reached algod"
    );
    drop(health);
    s.assert_nothing_submitted();
}
