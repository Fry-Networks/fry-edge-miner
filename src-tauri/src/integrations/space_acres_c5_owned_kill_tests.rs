//! c5 D13 — stopping SpaceAcres ends only SpaceAcres' own processes.
//!
//! BL8's tree kill (`taskkill /PID <pid> /T /F`) ends every descendant of the
//! tracked supervisor whatever its image. SpaceAcres 0.2.21 opens links in the
//! owner's browser from its GUI process (`open::that_detached`,
//! src/frontend/configuration.rs:776 and :838, src/frontend.rs:874 and :879),
//! so a browser it started is such a descendant, and toggling SpaceAcres off
//! closed it. `kill_tree_args` (and the BL8 pin on it) stays, off the stop path.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_tree_kill_never_ends_a_process_of_another_image() {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("fn kill_tree(pid: u32)")
        .expect("kill_tree must exist");
    let body = &code[at..at + code[at..].find("\n}\n").expect("must close")];
    assert!(
        !body.contains("kill_tree_args(") && !body.contains("\"/T\""),
        "taskkill /T ends every descendant, including a browser SpaceAcres opened:\n{body}"
    );
    assert!(
        body.contains("owned_kill_args("),
        "the kill must name only SpaceAcres-owned processes:\n{body}"
    );
}

#[test]
fn stop_falls_back_to_the_sweep_when_the_tree_could_not_be_listed() {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("async fn stop(&self) -> Result<()> {")
        .expect("stop must exist");
    let body = &code[at..at + code[at..].find("\n    }\n").expect("must close")];
    let kill = body
        .find("let tree_known = kill_tree(child.id());")
        .unwrap_or_else(|| panic!("stop() must keep kill_tree's answer:\n{body}"));
    let ret = kill
        + body[kill..]
            .find("return Ok(());")
            .expect("the tracked branch returns");
    assert!(
        body[kill..ret].matches("tree_known").count() >= 2,
        "stop() returns before the sweep even when the farmer could not be listed:\n{body}"
    );
}

use super::{owned_kill_args, owned_process_query, owned_tree_pids, parse_owned_rows};

fn rows(list: &[(u32, u32, &str)]) -> Vec<(u32, u32, String)> {
    list.iter()
        .map(|(pid, ppid, image)| (*pid, *ppid, image.to_string()))
        .collect()
}

#[test]
fn the_kill_names_the_supervisor_and_its_farmer_but_not_a_browser_it_opened() {
    let listed = rows(&[
        (100, 4, "space-acres.exe"),
        (200, 100, "space-acres-modern.exe"),
        (300, 200, "chrome.exe"),
        (301, 300, "chrome.exe"),
        (400, 4, "space-acres.exe"),
        (500, 7, "notepad.exe"),
    ]);
    assert_eq!(owned_tree_pids(100, &listed), vec![100, 200]);
}

#[test]
fn a_farmer_that_reuses_the_supervisors_image_is_found() {
    let listed = rows(&[(100, 4, "space-acres.exe"), (200, 100, "space-acres.exe")]);
    assert_eq!(owned_tree_pids(100, &listed), vec![100, 200]);
}

#[test]
fn a_farmer_orphaned_by_an_exited_supervisor_is_still_found() {
    let listed = rows(&[
        (200, 100, "space-acres-modern.exe"),
        (400, 4, "space-acres.exe"),
    ]);
    assert_eq!(owned_tree_pids(100, &listed), vec![200]);
}

#[test]
fn nothing_is_walked_through_a_process_of_another_image() {
    let listed = rows(&[
        (100, 4, "space-acres.exe"),
        (300, 100, "chrome.exe"),
        (600, 300, "space-acres.exe"),
    ]);
    assert_eq!(owned_tree_pids(100, &listed), vec![100]);
}

#[test]
fn a_parent_link_cycle_still_terminates() {
    let listed = rows(&[
        (100, 200, "space-acres.exe"),
        (200, 100, "space-acres-modern.exe"),
    ]);
    assert_eq!(owned_tree_pids(100, &listed), vec![100, 200]);
}

#[test]
fn image_names_match_whatever_their_case() {
    let listed = rows(&[
        (100, 4, "Space-Acres.exe"),
        (200, 100, "SPACE-ACRES-MODERN.EXE"),
    ]);
    assert_eq!(owned_tree_pids(100, &listed), vec![100, 200]);
}

#[test]
fn the_kill_arguments_force_exactly_the_named_pids_and_never_the_tree() {
    let args = owned_kill_args(&[100, 200]);
    assert_eq!(args, ["/F", "/PID", "100", "/PID", "200"]);
    assert!(!args.iter().any(|a| a.eq_ignore_ascii_case("/T")));
}

#[test]
fn listing_rows_parse_and_anything_malformed_is_skipped() {
    let listing = "100,4,space-acres.exe\r\n\n 200 , 100 ,space-acres-modern.exe\nx,1,foo\n300,1,\nWARNING: ignored\n";
    assert_eq!(
        parse_owned_rows(listing),
        rows(&[
            (100, 4, "space-acres.exe"),
            (200, 100, "space-acres-modern.exe")
        ])
    );
}

#[test]
fn the_listing_query_asks_for_both_images_and_their_parents() {
    let query = owned_process_query();
    for needle in [
        "Win32_Process",
        "'space-acres.exe'",
        "'space-acres-modern.exe'",
        "$_.ProcessId",
        "$_.ParentProcessId",
        "$_.Name",
    ] {
        assert!(query.contains(needle), "missing {needle}: {query}");
    }
    assert!(
        !query.contains('"'),
        "a double quote would need escaping on the command line: {query}"
    );
}
