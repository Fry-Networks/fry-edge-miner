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
    for t in targets:
        if os.path.isfile(t):
            yield t
        else:
            for root, _dirs, files in os.walk(t):
                for f in files:
                    if f.lower().endswith(PE_SUFFIXES):
                        yield os.path.join(root, f)


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
    for path in pe_files(argv[1:]):
        try:
            imps = imports_of(path)
        except Exception as exc:  # not a PE, or unreadable
            print(f"skip   {path}: {exc}")
            continue
        scanned += 1
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
