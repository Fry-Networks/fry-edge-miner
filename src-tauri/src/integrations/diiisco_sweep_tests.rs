//! The sweep of stale FEM-created candidate images (RC38 / D2): which images it
//! may remove, how listings and histories are read, and the candidate ledger.
//! Pure decisions only; the promotion wiring is tested in `rc38_tests`.
//! Synthetic ids and tags.
use super::super::diiisco_rebuild::{
    history_is_fem, ledger_add, ledger_read, ledger_remove, parse_image_listing, sweep_targets,
    ImageRef, SweepSets, LEDGER_FILE,
};

const NEW: &str = "sha256:11507a0e2f5e69d5dfa40a62a1bd7b6ee57e6bcd85c67c9b8431b36fff21c437";
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
        targets(
            &["sha256:021f8ffac9f9a70fd3468b23fc6f299877339299a4ae8764e892e3f197e867f5"],
            &[],
            &[],
            &[],
            &[]
        ),
        s(&["sha256:021f8ffac9f9a70fd3468b23fc6f299877339299a4ae8764e892e3f197e867f5"])
    );
}

#[test]
fn a_legacy_fem_tag_with_fem_history_goes() {
    let images = [img(
        "sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417",
        Some(FEM_TAG),
    )];
    assert_eq!(
        targets(
            &[],
            &images,
            &[],
            &[],
            &["sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417"]
        ),
        s(&["sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417"])
    );
}

#[test]
fn a_legacy_fem_tag_without_fem_history_stays() {
    let images = [img(
        "sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417",
        Some(FEM_TAG),
    )];
    assert!(targets(&[], &images, &[], &[], &[]).is_empty());
}

#[test]
fn an_image_with_two_fem_tags_is_removed_once() {
    let images = [
        img(
            "sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417",
            Some(FEM_TAG),
        ),
        img(
            "sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417",
            Some(FEM_TAG2),
        ),
    ];
    assert_eq!(
        targets(
            &["sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417"],
            &images,
            &[],
            &[],
            &["sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417"]
        ),
        s(&["sha256:c7c8fc91abb8ed76c29a883381b92612dcd8235d6e7ddca8d81f6e87d5fab417"])
    );
}

// ------------------------------------------------------ what never goes

#[test]
fn non_fem_images_are_never_removed() {
    let images = [
        img(
            "sha256:3bfc269594ef649228e9a74bab00f042efc91d5acc6fbee31a382e80d42388fe",
            Some("v1"),
        ),
        img(
            "sha256:dd7a946de80abf41797036335be60bf207f3262c6429f4993005d91f174ecf07",
            Some("experimental"),
        ),
        img(
            "sha256:f9b0078b5df596d2ea19010c001bbd009e651de2c57e8fb7e355f31eb9d3f739",
            Some("fem-0123"),
        ),
        img(
            "sha256:aee610558292023758a4229ddcf75f167c9904313a83cf795232ed7f7e2131c9",
            Some("fem-0123456789AB"),
        ),
        img(
            "sha256:d39d350b3835c7dd15040ae50eab49bc92ede7ef9c49230f38ff665147eb038c",
            None,
        ),
        // the same id also carries a tag that is not FEM's: the user's image
        img(
            "sha256:3f8fee624f43b2a9d685353269a0ab3eac785863ab6227636db1060fba1855e0",
            Some(FEM_TAG),
        ),
        img(
            "sha256:3f8fee624f43b2a9d685353269a0ab3eac785863ab6227636db1060fba1855e0",
            Some("mine"),
        ),
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
        img(
            "sha256:5e1e2bcac305958b27077ca136f35f0abae7cf38c9af678f7d220ed0cb51d4f8",
            Some("latest"),
        ),
        img(
            "sha256:5e1e2bcac305958b27077ca136f35f0abae7cf38c9af678f7d220ed0cb51d4f8",
            Some(FEM_TAG),
        ),
        img(
            "sha256:acba25512100f80b56fc3ccd14c65be55d94800cda77585c5f41a887e398f9be",
            Some(FEM_TAG),
        ),
        img(
            "sha256:8322e87d24582952e5c996da182de0eca66dd2bf58fba5ec56a325c656f21b38",
            Some(FEM_TAG2),
        ),
        img(NEW, Some("fem-aaaaaaaaaaaa")),
    ];
    let ledger = [
        "sha256:5e1e2bcac305958b27077ca136f35f0abae7cf38c9af678f7d220ed0cb51d4f8",
        "sha256:acba25512100f80b56fc3ccd14c65be55d94800cda77585c5f41a887e398f9be",
        "sha256:8322e87d24582952e5c996da182de0eca66dd2bf58fba5ec56a325c656f21b38",
        NEW,
    ];
    let fem = [
        "sha256:5e1e2bcac305958b27077ca136f35f0abae7cf38c9af678f7d220ed0cb51d4f8",
        "sha256:acba25512100f80b56fc3ccd14c65be55d94800cda77585c5f41a887e398f9be",
        "sha256:8322e87d24582952e5c996da182de0eca66dd2bf58fba5ec56a325c656f21b38",
        NEW,
    ];
    assert!(targets(
        &ledger,
        &images,
        &[
            "sha256:8322e87d24582952e5c996da182de0eca66dd2bf58fba5ec56a325c656f21b38",
            "sha256:acba25512100f80b56fc3ccd14c65be55d94800cda77585c5f41a887e398f9be"
        ],
        &["sha256:acba25512100f80b56fc3ccd14c65be55d94800cda77585c5f41a887e398f9be"],
        &fem
    )
    .is_empty());
}

#[test]
fn an_image_used_by_a_stopped_container_stays_but_its_unused_sibling_goes() {
    let images = [
        img(
            "sha256:8322e87d24582952e5c996da182de0eca66dd2bf58fba5ec56a325c656f21b38",
            Some(FEM_TAG),
        ),
        img(
            "sha256:febe1d741b49e5a9c31526728d8c5134a803adfc4c04c4f052673722ed85597e",
            Some(FEM_TAG2),
        ),
    ];
    let fem = [
        "sha256:8322e87d24582952e5c996da182de0eca66dd2bf58fba5ec56a325c656f21b38",
        "sha256:febe1d741b49e5a9c31526728d8c5134a803adfc4c04c4f052673722ed85597e",
    ];
    assert_eq!(
        targets(
            &[],
            &images,
            &["sha256:8322e87d24582952e5c996da182de0eca66dd2bf58fba5ec56a325c656f21b38"],
            &[],
            &fem
        ),
        s(&["sha256:febe1d741b49e5a9c31526728d8c5134a803adfc4c04c4f052673722ed85597e"])
    );
}

// --------------------------------------------------------------- parsing

#[test]
fn the_image_listing_is_read_per_line_and_untagged_entries_have_no_tag() {
    let text = "sha256:9834876dcfb05cb167a5c24953eba58c4ac89b1adf57f28f2f9d09af107ee8f0 latest\nsha256:3e744b9dc39389baf0c5a0660589b8402f3dbb49b89b3e75f2c9355852a3c677 fem-0123456789ab\nsha256:64daa44ad493ff28a96effab6e77f1732a3d97d83241581b37dbd70a7a4900fe <none>\n\n  sha256:730f75dafd73e047b86acb2dbd74e75dcb93272fa084a9082848f2341aa1abb6   v1  \nmalformed\n";
    assert_eq!(
        parse_image_listing(text),
        vec![
            img(
                "sha256:9834876dcfb05cb167a5c24953eba58c4ac89b1adf57f28f2f9d09af107ee8f0",
                Some("latest")
            ),
            img(
                "sha256:3e744b9dc39389baf0c5a0660589b8402f3dbb49b89b3e75f2c9355852a3c677",
                Some(FEM_TAG)
            ),
            img(
                "sha256:64daa44ad493ff28a96effab6e77f1732a3d97d83241581b37dbd70a7a4900fe",
                None
            ),
            img(
                "sha256:730f75dafd73e047b86acb2dbd74e75dcb93272fa084a9082848f2341aa1abb6",
                Some("v1")
            ),
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
    ledger_add(
        d.path(),
        "sha256:7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed",
    )
    .unwrap();
    ledger_add(
        d.path(),
        "sha256:3fc4ccfe745870e2c0d99f71f30ff0656c8dedd41cc1d7d3d376b0dbe685e2f3",
    )
    .unwrap();
    ledger_add(
        d.path(),
        "sha256:7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed",
    )
    .unwrap();
    assert_eq!(
        ledger_read(d.path()),
        s(&[
            "sha256:7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed",
            "sha256:3fc4ccfe745870e2c0d99f71f30ff0656c8dedd41cc1d7d3d376b0dbe685e2f3"
        ])
    );
    ledger_remove(
        d.path(),
        "sha256:7692c3ad3540bb803c020b3aee66cd8887123234ea0c6e7143c0add73ff431ed",
    );
    assert_eq!(
        ledger_read(d.path()),
        s(&["sha256:3fc4ccfe745870e2c0d99f71f30ff0656c8dedd41cc1d7d3d376b0dbe685e2f3"])
    );
    ledger_remove(
        d.path(),
        "sha256:5ad38304b535c2987dbd24657c1a11b884984ff600d9f389deb0d4e634fee792",
    );
    assert_eq!(
        ledger_read(d.path()),
        s(&["sha256:3fc4ccfe745870e2c0d99f71f30ff0656c8dedd41cc1d7d3d376b0dbe685e2f3"])
    );
}

#[test]
fn the_ledger_ignores_lines_that_are_not_image_ids_and_leaves_no_temp_file() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join(LEDGER_FILE),
        "sha256:2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df\nnot an id\n\n$(rm -rf x)\nsha256:2a5de9a3a0c5c8749a90ad51c994991354aef84ed18cf4352333261f2a233742\n",
    )
    .unwrap();
    assert_eq!(
        ledger_read(d.path()),
        s(&[
            "sha256:2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df",
            "sha256:2a5de9a3a0c5c8749a90ad51c994991354aef84ed18cf4352333261f2a233742"
        ])
    );
    ledger_add(
        d.path(),
        "sha256:80bd58cc6d5d42da53d0070dae5727a1eab31bb5955bf38c25f4e0b064eb93c1",
    )
    .unwrap();
    let names: Vec<String> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec![LEDGER_FILE.to_string()], "{names:?}");
}

// ------------------------------------------------- image ids are sha256:<64 hex>

fn bad_ids() -> Vec<String> {
    let h = "a".repeat(64);
    vec![
        "sha256:ab".to_string(),
        "sha256:".to_string(),
        format!("sha256:{}", h.to_uppercase()),
        format!("sha256:{}", &h[..63]),
        format!("sha256:{h}a"),
        format!("sha256:{}g", &h[..63]),
        format!("sha256:{}-", &h[..63]),
        format!("sha512:{h}"),
        h.clone(),
        format!(" sha256:{h}x"),
    ]
}

#[test]
fn a_valid_id_round_trips_through_the_ledger() {
    // guards the tests below against passing vacuously (invalid ids are dropped)
    let d = tempfile::tempdir().unwrap();
    let id = format!("sha256:{}", "0123456789abcdef".repeat(4));
    ledger_add(d.path(), &id).unwrap();
    assert_eq!(ledger_read(d.path()), vec![id]);
}

#[test]
fn ledger_add_drops_every_malformed_id() {
    for bad in bad_ids() {
        let d = tempfile::tempdir().unwrap();
        ledger_add(d.path(), &bad).unwrap();
        assert!(ledger_read(d.path()).is_empty(), "accepted {bad:?}");
    }
}

#[test]
fn a_ledger_file_with_malformed_lines_yields_nothing() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(LEDGER_FILE), bad_ids().join("\n")).unwrap();
    assert!(ledger_read(d.path()).is_empty());
}

#[test]
fn malformed_ledger_ids_are_never_sweep_targets() {
    let bad = bad_ids();
    let got = sweep_targets(
        &SweepSets {
            ledger: &bad,
            images: &[],
            containers: &[],
            running: &[],
            new_id: NEW,
        },
        &|_: &str| true,
    );
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn malformed_listed_ids_are_not_parsed() {
    for bad in bad_ids() {
        let text = format!("{bad} {FEM_TAG}\n");
        assert!(parse_image_listing(&text).is_empty(), "accepted {bad:?}");
    }
}
