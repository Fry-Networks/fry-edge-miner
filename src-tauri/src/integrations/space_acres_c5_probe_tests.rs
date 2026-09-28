//! c5 D5 — the liveness probe must see every SpaceAcres process image.
//!
//! It matched only `space-acres.exe`. SpaceAcres 0.2.21's supervisor runs its
//! farmer as `space-acres-modern.exe --child-process` (upstream main.rs:597-627
//! derives the name), so a farmer left running without its supervisor read as
//! "not running" and FEM started a second one: two farmers ran at once.

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_liveness_probe_matches_every_space_acres_image() {
    let code = code_only(include_str!("space_acres.rs"));
    let at = code
        .find("fn image_name_probe() -> bool {")
        .expect("the image-name probe must exist");
    let body = &code[at..at + code[at..].find("\n    }\n").expect("must close")];
    assert!(
        body.contains("tasklist_shows_space_acres("),
        "the probe must match the farmer image as well as the supervisor's:\n{body}"
    );
}

// Rows in `tasklist`'s own layout.
const HEADER: &str =
    "Image Name                     PID Session Name        Session#    Mem Usage\n\
                      ========================= ======== ================ =========== ============";
const SUPERVISOR: &str =
    "space-acres.exe               4120 Console                    1     18,532 K";
const FARMER: &str = "space-acres-modern.exe        4188 Console                    1    912,004 K";
const BROWSER: &str =
    "chrome.exe                    5210 Console                    1    210,440 K";

fn listing(rows: &[&str]) -> String {
    let mut out = HEADER.to_string();
    for row in rows {
        out.push('\n');
        out.push_str(row);
    }
    out
}

#[test]
fn a_farmer_running_without_its_supervisor_counts_as_running() {
    assert!(super::tasklist_shows_space_acres(&listing(&[
        BROWSER, FARMER
    ])));
}

#[test]
fn the_supervisor_alone_counts_as_running() {
    assert!(super::tasklist_shows_space_acres(&listing(&[
        SUPERVISOR, BROWSER
    ])));
}

#[test]
fn the_image_match_ignores_case() {
    assert!(super::tasklist_shows_space_acres(
        &listing(&[FARMER]).to_uppercase()
    ));
}

#[test]
fn a_listing_without_space_acres_is_not_running() {
    assert!(!super::tasklist_shows_space_acres(&listing(&[BROWSER])));
    assert!(!super::tasklist_shows_space_acres(""));
}
