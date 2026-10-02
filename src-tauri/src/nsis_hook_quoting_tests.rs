//! B4: the installer's path-scoped kill must not interpolate a path into the
//! PowerShell it runs.
//!
//! NSIS expands `$INSTDIR` / `$APPDATA` / `$LOCALAPPDATA` into the `-Command`
//! string verbatim, with no escaping. A profile folder containing an apostrophe
//! — O'Brien, D'Angelo, which is what a Microsoft account with that surname
//! produces — closes the single-quoted pattern early, the whole command fails
//! to parse, `Pop $1` discards the failure, and the installer walks into the
//! "Error opening file for writing" failure this hook exists to prevent.
//!
//! That is a REGRESSION this repo introduced: the bare `taskkill /F /T /IM`
//! calls it replaced had no quoting to break. The path scoping that replaced
//! them is still right and must not be given up to fix this — killing by bare
//! image name is what was force-killing users' own unrelated Space Acres,
//! Titan and MystNodes installs.
//!
//! Source-level, because the property is decidable from the file: a path that
//! never appears in the script text cannot break its quoting, whatever the
//! path contains. Whether the environment actually reaches the child is an
//! installer-run question and is covered in the VM matrix, not here.

const HOOK: &str = include_str!("../nsis-hooks.nsh");

/// The one command line that does the killing.
///
/// Selected from NON-COMMENT lines only. `;` starts an NSIS comment, but the
/// PowerShell itself contains semicolons as statement separators, so this
/// filters whole comment LINES rather than truncating at the first `;` — and
/// the comments in this hook discuss `$INSTDIR` by name, which would otherwise
/// satisfy the assertions below from prose.
fn kill_command() -> &'static str {
    HOOK.lines()
        .find(|l| !l.trim_start().starts_with(';') && l.contains("Get-CimInstance"))
        .expect("the hook must still stop partner processes that hold the install folder")
}

#[test]
fn the_hook_never_interpolates_a_path_into_the_powershell_command() {
    let cmd = kill_command();
    for var in ["$INSTDIR", "$APPDATA", "$LOCALAPPDATA"] {
        assert!(
            !cmd.contains(var),
            "nsis-hooks.nsh interpolates {var} into the PowerShell command. NSIS \
             expands it verbatim with no escaping, so a profile folder like \
             C:\\Users\\O'Brien closes the quoted pattern early and the whole \
             -Command fails to parse — the installer then kills nothing and hits \
             the locked-file failure this hook exists to prevent. Pass the path \
             through the environment instead.\n\n{cmd}"
        );
    }
}

#[test]
fn the_hook_passes_paths_through_the_environment_instead() {
    for var in [
        "FEM_INSTDIR",
        "FEM_PARTNERS_APPDATA",
        "FEM_PARTNERS_LOCALAPPDATA",
    ] {
        assert!(
            HOOK.contains(&format!("SetEnvironmentVariable(t \"{var}\"")),
            "the hook must set {var} before the call, so the path never enters \
             the script text"
        );
        assert!(
            kill_command().contains(&format!("env:{var}")),
            "the command must read {var} from the environment"
        );
    }
    // Reading `$env:X` into a PowerShell variable is the entire point: it puts
    // no quote character around the path. Wrapping it in quotes again would
    // reintroduce exactly the defect.
    let cmd = kill_command();
    for quoted in ["'$env:", "\"$env:", "'$$env:", "\"$$env:"] {
        assert!(
            !cmd.contains(quoted),
            "the environment read is wrapped in quotes ({quoted}), which \
             reintroduces the quoting this change exists to remove:\n\n{cmd}"
        );
    }
}

/// ANTI-WEAKENING. The scoping IS the B4 fix. Fixing the quoting must not cost
/// it — a hook that kills by bare image name again would force-kill a user's
/// own unrelated installs, which is the bug the scoping was introduced for.
#[test]
fn the_kill_is_still_scoped_to_fems_own_directories() {
    let cmd = kill_command();
    assert!(
        cmd.contains("ExecutablePath"),
        "the filter must still select on image PATH:\n\n{cmd}"
    );
    assert!(
        cmd.matches("-like").count() >= 3,
        "all three roots — install tree, roaming partners, local partners — \
         must still be matched:\n\n{cmd}"
    );
    for image in [
        "frynode.exe",
        "titan-edge.exe",
        "sdk_client.exe",
        "space-acres.exe",
    ] {
        assert!(
            cmd.contains(image),
            "{image} must still be in the image filter:\n\n{cmd}"
        );
    }
    for image in [
        "frynode.exe",
        "titan-edge.exe",
        "sdk_client.exe",
        "space-acres.exe",
    ] {
        assert!(
            !HOOK.contains(&format!("/IM {image}")),
            "the hook must not go back to killing {image} by bare image name"
        );
    }
}
