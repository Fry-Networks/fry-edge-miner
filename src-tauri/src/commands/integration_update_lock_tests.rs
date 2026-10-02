//! FAIL-4 / FAIL-5 (pre-tag review of 0.4.34).
//!
//! FAIL-4: `install_update` held the global registry `std::sync::Mutex` across
//! `Handle::block_on(integration.apply_update(..))`. B21 made SpaceAcres'
//! `apply_update` do real work (download, installer, restart), and every health
//! loop, the PoC reporter and `get_integrations` take that lock synchronously on
//! async workers, so a long update pinned every worker and nothing drove tokio's
//! IO/timer driver again. The guard must be dropped before the first
//! `block_on`.
//!
//! FAIL-5: B21's only unit proof never touched `apply_update`. The guard test
//! below pins that the update forces the reinstall, restarts the farmer, and
//! that the reinstall actually honours `force`.
//!
//! The command needs a live `tauri::State<AppState>` and the update a real
//! download and installer, so these read the real source.

const UPDATES: &str = include_str!("updates.rs");
const SPACE_ACRES: &str = include_str!("../integrations/space_acres.rs");

fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Byte offset of the `}` that closes the innermost block containing `at`.
fn enclosing_block_end(code: &str, at: usize) -> usize {
    let bytes = code.as_bytes();
    let mut depth = 0i32;
    let mut i = at;
    // Walk back to the innermost unmatched `{`.
    let open = loop {
        if i == 0 {
            panic!("no enclosing block for offset {at}");
        }
        i -= 1;
        match bytes[i] {
            b'}' => depth += 1,
            b'{' if depth == 0 => break i,
            b'{' => depth -= 1,
            _ => {}
        }
    };
    let mut depth = 0i32;
    for (j, b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return j;
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced block opened at {open}");
}

/// True when every registry lock in `branch` lives in a block that closes
/// before the branch's first `block_on(`.
fn guard_released_before_block_on(branch: &str) -> bool {
    let first_block_on = branch
        .find("block_on(")
        .expect("control: the branch must drive the update with block_on");
    let locks: Vec<usize> = branch
        .match_indices("registry.lock()")
        .map(|(i, _)| i)
        .collect();
    assert!(!locks.is_empty(), "control: the branch must take the lock");
    locks
        .iter()
        .all(|&at| enclosing_block_end(branch, at) < first_block_on)
}

/// The `kind == "integration"` branch of `install_update`, comments stripped.
fn integration_branch() -> String {
    let code = code_only(UPDATES);
    let at = code
        .find(&format!("kind == \"integr{}\"", "ation"))
        .expect("install_update's integration branch must exist");
    let end = code[at..]
        .find("} else {")
        .map(|e| at + e)
        .expect("the integration branch must be followed by an else");
    code[at..end].to_string()
}

#[test]
fn the_registry_guard_is_dropped_before_the_update_is_driven() {
    // Positive and negative controls for the predicate itself, in this run.
    let good = "if x { block_in_place(|| { let i = { let reg = state.registry.lock()?; \
                reg.get(&id)? }; rt.block_on(i.apply_update(&v)); }) }";
    let bad = "if x { block_in_place(|| { let reg = state.registry.lock()?; \
               let i = reg.get(&id)?; rt.block_on(i.apply_update(&v)); }) }";
    let temp = "if x { block_in_place(|| { rt.block_on(state.registry.lock()?.get(&id)?\
                .apply_update(&v)); }) }";
    assert!(
        guard_released_before_block_on(good),
        "control: scoped guard"
    );
    assert!(!guard_released_before_block_on(bad), "control: held guard");
    assert!(
        !guard_released_before_block_on(temp),
        "control: temporary guard"
    );

    let branch = integration_branch();
    assert!(
        branch.contains("apply_update("),
        "control: wrong branch:\n{branch}"
    );
    assert!(
        guard_released_before_block_on(&branch),
        "FAIL-4: install_update holds the registry guard across block_on, so a long \
         integration update (B21's SpaceAcres reinstall) pins every async worker that \
         takes the same lock:\n{branch}"
    );
}

/// `name`'s body in `code`: from `fn name(` to its first line that closes at
/// the given indentation.
fn fn_body<'a>(code: &'a str, sig: &str, close: &str) -> &'a str {
    let at = code.find(sig).unwrap_or_else(|| panic!("{sig} must exist"));
    let end = code[at..]
        .find(close)
        .map(|e| at + e)
        .unwrap_or_else(|| panic!("{sig} must close"));
    &code[at..end]
}

/// GUARD (green before and after): B21's update reinstalls with force and
/// restarts the farmer, and the reinstall honours force. Kills the mutants
/// `install_impl(false)`, a deleted `start()`, and force dropped inside
/// `install_impl`.
#[test]
fn space_acres_update_forces_the_reinstall_and_restarts() {
    let code = code_only(SPACE_ACRES);
    let update = fn_body(&code, "async fn apply_update(", "\n    }\n");
    let reinstall = update
        .find("self.install_impl(true)")
        .unwrap_or_else(|| panic!("FAIL-5: apply_update must force the reinstall:\n{update}"));
    let restart = update
        .find("self.start()")
        .unwrap_or_else(|| panic!("FAIL-5: apply_update must restart the farmer:\n{update}"));
    assert!(
        reinstall < restart,
        "FAIL-5: the farmer must restart after the reinstall:\n{update}"
    );

    let install = fn_body(&code, "async fn install_impl(", "\n    }\n");
    assert!(
        install.contains("install_short_circuits(force,"),
        "FAIL-5: install_impl must pass force to the short-circuit guard:\n{install}"
    );
    assert!(
        install.contains("&& !force"),
        "FAIL-5: install_impl's existing-binary check must honour force:\n{install}"
    );
}
