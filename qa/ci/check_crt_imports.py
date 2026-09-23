#!/usr/bin/env python3
"""Fail the build if any shipped PE imports the MSVC C/C++ runtime.

B1: v0.4.28/v0.4.29 shipped a binary that imported no CRT DLL; from v0.4.30 the
release binary gained VCRUNTIME140.dll + VCRUNTIME140_1.dll, so every machine
without the VC++ redistributable hit
"VCRUNTIME140_1.dll was not found" and could not start the app.

This gate is deliberately dumb and absolute: no PE that we build or bundle may
import a redistributable CRT DLL. Run it over the bundle directory (and over any
downloaded release asset) before publishing.

Usage:  check_crt_imports.py <file-or-dir> [...]
Exit 0 = clean, 1 = a forbidden import was found, 2 = usage/parse error.
"""
import sys
import os
import pefile

FORBIDDEN_PREFIXES = ("VCRUNTIME", "MSVCP", "CONCRT", "MSVCR")
# Debug CRTs must never ship either; they also imply a debug build.
FORBIDDEN_EXACT = ("UCRTBASED.DLL", "VCRUNTIME140D.DLL", "MSVCP140D.DLL")
PE_SUFFIXES = (".exe", ".dll", ".sys", ".node")


def pe_files(targets):
    """Yield every PE under `targets`.

    A target that names a FILE must exist. Without that check a path that has
    been renamed or moved falls through to `os.walk`, which yields nothing for a
    non-existent directory SILENTLY — so the remaining targets still produce PEs,
    `scanned` stays above zero, and the gate prints PASSED without ever having
    looked at the binary it exists to protect. Reproduced: invoked with a missing
    app exe plus one unrelated DLL, the gate reported
    "CRT-IMPORT GATE PASSED: 1 PE file(s)" and exit 0.

    The `scanned == 0` guard in main() does not cover this, because it only fires
    when NOTHING was scanned. This is the same failure shape as an uncontrolled
    zero: absence of a finding read as a finding of absence.
    """
    for t in targets:
        if os.path.isfile(t):
            yield t
        elif os.path.isdir(t):
            for root, _dirs, files in os.walk(t):
                for f in files:
                    if f.lower().endswith(PE_SUFFIXES):
                        yield os.path.join(root, f)
        else:
            raise MissingTarget(t)


class MissingTarget(Exception):
    """A path handed to the gate does not exist at all."""


def imports_of(path):
    pe = pefile.PE(path, fast_load=True)
    pe.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]]
    )
    return sorted(
        e.dll.decode(errors="replace") for e in getattr(pe, "DIRECTORY_ENTRY_IMPORT", [])
    )


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    scanned = 0
    violations = []
    # Named-file targets are the ones a rename can silently drop; require each
    # to be scanned by NAME, so "the app exe was inspected" is asserted rather
    # than assumed from a non-zero count.
    required = [t for t in argv[1:] if t.lower().endswith(PE_SUFFIXES)]
    seen = set()
    try:
        candidates = list(pe_files(argv[1:]))
    except MissingTarget as missing:
        print(
            f"ERROR: target does not exist: {missing}\n"
            "Refusing to report a result. A missing path would otherwise be walked as an "
            "empty directory and the gate would print PASSED without inspecting it."
        )
        return 2
    for path in candidates:
        try:
            imps = imports_of(path)
        except Exception as exc:  # not a PE, or unreadable
            print(f"skip   {path}: {exc}")
            continue
        scanned += 1
        seen.add(os.path.realpath(path))
        bad = [
            d
            for d in imps
            if d.upper().startswith(FORBIDDEN_PREFIXES) or d.upper() in FORBIDDEN_EXACT
        ]
        if bad:
            violations.append((path, bad))
            print(f"FAIL   {path}: imports {bad}")
        else:
            print(f"ok     {path}: no CRT imports ({len(imps)} imports)")
    if scanned == 0:
        print("ERROR: no PE files were scanned — check the path argument")
        return 2
    unscanned = [t for t in required if os.path.realpath(t) not in seen]
    if unscanned:
        print(
            "\nERROR: these explicitly named PE files were never scanned: "
            f"{unscanned}\nThe gate refuses to pass on a partial scan."
        )
        return 2
    if violations:
        print(
            f"\nCRT-IMPORT GATE FAILED: {len(violations)} of {scanned} PE file(s) import a "
            f"redistributable CRT.\nShip a binary that links the CRT statically "
            f"(+crt-static) instead; users without the VC++ redistributable cannot start it."
        )
        return 1
    print(f"\nCRT-IMPORT GATE PASSED: {scanned} PE file(s), none import a redistributable CRT.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
