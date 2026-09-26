//! c4 BUG LOOP 9 (B11): `install_impl` must READ-AND-RESET
//! `next_install_is_user_gesture` — `.swap(false, ..)` — not merely read it.
//!
//! d65a017 retargeted `the_swap_true_branch_maps_to_user_click_and_false_to_automatic`
//! to `install_trigger_for` and with that dropped its `.swap(false,` needle, so
//! `.load(..)` (the flag is never reset) and `.swap(true, ..)` (the flag is set
//! for good) both survived every test. Either hands the NEXT automatic
//! `install()` (boot recovery, the health loop) the UserClick authority of an
//! earlier user gesture — the row-10 hazard the gate exists to prevent.
//!
//! The only read of the flag is inside `install_impl`'s
//! `#[cfg(target_os = "windows")]` block, so on the Linux test host it cannot
//! be executed; like `space_acres_elevation_gate_tests`, this pins the source.

const SPACE_ACRES_SRC: &str = include_str!("space_acres.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One function's body, by brace balance (the helper the elevation gate
/// tests use, duplicated since each `#[path]` test file is self-contained).
fn fn_body<'a>(code: &'a str, signature_needle: &str) -> &'a str {
    let at = code
        .find(signature_needle)
        .unwrap_or_else(|| panic!("signature not found: {signature_needle}"));
    let open = at
        + signature_needle
            .rfind('{')
            .expect("signature needle must end at the opening brace");
    let bytes = code.as_bytes();
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &code[at..=i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("no matching closing brace for: {signature_needle}");
}

/// `install_impl`'s body with all whitespace removed, so rustfmt's line
/// breaks inside the method chain cannot move a needle.
fn install_impl_squashed() -> String {
    let code = code_only(SPACE_ACRES_SRC);
    fn_body(
        &code,
        "async fn install_impl(&self, force: bool) -> Result<()> {",
    )
    .chars()
    .filter(|c| !c.is_whitespace())
    .collect()
}

#[test]
fn install_impl_reads_and_resets_the_gesture_flag_into_the_trigger() {
    let body = install_impl_squashed();
    assert!(
        body.contains("letuser_gesture=self.next_install_is_user_gesture.swap(false,"),
        "install_impl must take the flag with `.swap(false, ..)` — read it AND reset it — \
         as the value it maps to a trigger; a plain read or `.swap(true, ..)` leaves a \
         past user gesture armed for the next automatic install: {body}"
    );
    assert!(
        !body.contains("next_install_is_user_gesture.load(")
            && !body.contains("next_install_is_user_gesture.swap(true,"),
        "the flag must never be read without being reset to false: {body}"
    );
}

#[test]
fn install_impl_touches_the_gesture_flag_exactly_once() {
    let body = install_impl_squashed();
    assert_eq!(
        body.matches("next_install_is_user_gesture").count(),
        1,
        "one atomic read-and-reset, and no second read or write that could re-arm it: {body}"
    );
}
