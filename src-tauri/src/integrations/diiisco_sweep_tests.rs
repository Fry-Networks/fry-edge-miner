//! The sweep of stale FEM-created candidate images (RC38 / D2): which images it
//! may remove, how listings and histories are read, and the candidate ledger.
//! Pure decisions only; the promotion wiring is tested in `rc38_tests`.
//! Synthetic ids and tags.
use super::super::diiisco_rebuild::{
    history_is_fem, ledger_add, ledger_read, ledger_remove, parse_image_listing, sweep_targets,
    ImageRef, SweepSets, LEDGER_FILE,
};

const NEW: &str = "sha256:new";
const FEM_TAG: &str = "fem-0123456789ab";
const FEM_TAG2: &str = "fem-ba9876543210";

fn img(id: &str, tag: Option<&str>) -> ImageRef {
    ImageRef {
        id: id.into(),
        tag: tag.map(str::to_string),
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// Decide with the given evidence ids; everything else is "not built by FEM".
fn targets(
    ledger: &[&str],
    images: &[ImageRef],
    containers: &[&str],
    running: &[&str],
    fem: &[&str],
) -> Vec<String> {
    let (ledger, containers, running) = (s(ledger), s(containers), s(running));
    let fem = s(fem);
    sweep_targets(
        &SweepSets {
            ledger: &ledger,
            images,
            containers: &containers,
            running: &running,
            new_id: NEW,
        },
        &|id: &str| fem.iter().any(|f| f == id),
    )
}

// ------------------------------------------------------------- what goes

#[test]
fn a_ledger_candidate_goes() {
    assert_eq!(
        targets(&["sha256:led"], &[], &[], &[], &[]),
        s(&["sha256:led"])
    );
}

#[test]
fn a_legacy_fem_tag_with_fem_history_goes() {
    let images = [img("sha256:leg", Some(FEM_TAG))];
    assert_eq!(
        targets(&[], &images, &[], &[], &["sha256:leg"]),
        s(&["sha256:leg"])
    );
}

#[test]
fn a_legacy_fem_tag_without_fem_history_stays() {
    let images = [img("sha256:leg", Some(FEM_TAG))];
    assert!(targets(&[], &images, &[], &[], &[]).is_empty());
}

#[test]
fn an_image_with_two_fem_tags_is_removed_once() {
    let images = [
        img("sha256:leg", Some(FEM_TAG)),
        img("sha256:leg", Some(FEM_TAG2)),
    ];
    assert_eq!(
        targets(&["sha256:leg"], &images, &[], &[], &["sha256:leg"]),
        s(&["sha256:leg"])
    );
}

// ------------------------------------------------------ what never goes

#[test]
fn non_fem_images_are_never_removed() {
    let images = [
        img("sha256:v1", Some("v1")),
        img("sha256:exp", Some("experimental")),
        img("sha256:short", Some("fem-0123")),
        img("sha256:upper", Some("fem-0123456789AB")),
        img("sha256:dang", None),
        // the same id also carries a tag that is not FEM's: the user's image
        img("sha256:mixed", Some(FEM_TAG)),
        img("sha256:mixed", Some("mine")),
    ];
    let all: Vec<&str> = images.iter().map(|i| i.id.as_str()).collect();
    assert!(
        targets(&[], &images, &[], &[], &all).is_empty(),
        "even with FEM-looking history none of these is attributable"
    );
}

#[test]
fn protected_images_are_never_removed_in_either_class() {
    let images = [
        img("sha256:latest", Some("latest")),
        img("sha256:latest", Some(FEM_TAG)),
        img("sha256:run", Some(FEM_TAG)),
        img("sha256:stopped", Some(FEM_TAG2)),
        img(NEW, Some("fem-aaaaaaaaaaaa")),
    ];
    let ledger = ["sha256:latest", "sha256:run", "sha256:stopped", NEW];
    let fem = ["sha256:latest", "sha256:run", "sha256:stopped", NEW];
    assert!(targets(
        &ledger,
        &images,
        &["sha256:stopped", "sha256:run"],
        &["sha256:run"],
        &fem
    )
    .is_empty());
}

#[test]
fn an_image_used_by_a_stopped_container_stays_but_its_unused_sibling_goes() {
    let images = [
        img("sha256:stopped", Some(FEM_TAG)),
        img("sha256:unused", Some(FEM_TAG2)),
    ];
    let fem = ["sha256:stopped", "sha256:unused"];
    assert_eq!(
        targets(&[], &images, &["sha256:stopped"], &[], &fem),
        s(&["sha256:unused"])
    );
}

// --------------------------------------------------------------- parsing

#[test]
fn the_image_listing_is_read_per_line_and_untagged_entries_have_no_tag() {
    let text = "sha256:aaa latest\nsha256:bbb fem-0123456789ab\nsha256:ccc <none>\n\n  sha256:ddd   v1  \nmalformed\n";
    assert_eq!(
        parse_image_listing(text),
        vec![
            img("sha256:aaa", Some("latest")),
            img("sha256:bbb", Some(FEM_TAG)),
            img("sha256:ccc", None),
            img("sha256:ddd", Some("v1")),
        ]
    );
    assert!(parse_image_listing("").is_empty());
}

#[test]
fn only_a_history_with_all_three_fem_dockerfile_marks_is_fem() {
    let full = "/bin/sh -c #(nop) CMD [\"node\" \"dist/dev.js\"]\n\
                /bin/sh -c mkdir -p /app/data\n\
                COPY /build/node_modules ./node_modules # buildkit";
    assert!(history_is_fem(full));
    for drop in ["dist/dev.js", "mkdir -p /app/data", "/build/node_modules"] {
        assert!(!history_is_fem(&full.replace(drop, "x")), "without {drop}");
    }
    assert!(!history_is_fem(""));
    assert!(!history_is_fem("node:22-slim"));
}

// ---------------------------------------------------------------- ledger

#[test]
fn the_ledger_round_trips_dedupes_and_removes() {
    let d = tempfile::tempdir().unwrap();
    assert!(ledger_read(d.path()).is_empty(), "no file = empty");
    ledger_add(d.path(), "sha256:one").unwrap();
    ledger_add(d.path(), "sha256:two").unwrap();
    ledger_add(d.path(), "sha256:one").unwrap();
    assert_eq!(ledger_read(d.path()), s(&["sha256:one", "sha256:two"]));
    ledger_remove(d.path(), "sha256:one");
    assert_eq!(ledger_read(d.path()), s(&["sha256:two"]));
    ledger_remove(d.path(), "sha256:absent");
    assert_eq!(ledger_read(d.path()), s(&["sha256:two"]));
}

#[test]
fn the_ledger_ignores_lines_that_are_not_image_ids_and_leaves_no_temp_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join(LEDGER_FILE),
        "sha256:ok\nnot an id\n\n$(rm -rf x)\nsha256:ok2\n",
    )
    .unwrap();
    assert_eq!(ledger_read(d.path()), s(&["sha256:ok", "sha256:ok2"]));
    ledger_add(d.path(), "sha256:ok3").unwrap();
    let names: Vec<String> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![LEDGER_FILE.to_string()], "{names:?}");
}
