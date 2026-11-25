//! U6 RED->GREEN tests for FEM-DISCORD-PORT8088-WINGBITS.

const FRYVPN_RS: &str = include_str!("fryvpn.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn fryvpn_supports_fallback_api_port_and_tracks_active_port() {
    let code = code_only(FRYVPN_RS);
    assert!(
        code.contains("ACTIVE_API_PORT"),
        "fryvpn.rs must track the chosen frynode API port in ACTIVE_API_PORT"
    );
    assert!(
        code.contains("fn select_api_port("),
        "fryvpn.rs must provide select_api_port for fallback selection"
    );
    assert!(
        code.contains("8120"),
        "fryvpn.rs must search fallback ports up to 8120 when 8088 is occupied"
    );
    assert!(
        !code.contains("\"http://127.0.0.1:8088/health\""),
        "probe_health_once must not hardcode port 8088"
    );
}

#[test]
fn select_api_port_picks_fallback_when_8088_is_occupied_and_none_if_all_busy() {
    use super::fryvpn::select_api_port;

    assert_eq!(
        select_api_port(8088, 8089..=8120, |_| true),
        Some(8088),
        "preferred 8088 wins when free"
    );
    assert_eq!(
        select_api_port(8088, 8089..=8120, |p| p != 8088),
        Some(8089),
        "Wingbits holding 8088 falls back to 8089"
    );
    assert_eq!(
        select_api_port(8088, 8089..=8120, |p| p == 8120),
        Some(8120),
        "upper end of fallback range 8120 is reachable"
    );
    assert_eq!(
        select_api_port(8088, 8089..=8120, |_| false),
        None,
        "all ports occupied returns None"
    );
}
