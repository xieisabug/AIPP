"""Locate missing PE import entrypoints for a binary (diagnose 0xc0000139)."""
import os
import sys

import pefile


def find_dll(name: str, exe_dir: str) -> str | None:
    candidates = [
        exe_dir,
        r"C:\Windows\System32",
        r"C:\Windows\SysWOW64",
        r"C:\Windows\System",
    ]
    for directory in candidates:
        path = os.path.join(directory, name)
        if os.path.exists(path):
            return path
    # api-ms-win-* / vcruntime forwarders live in System32 downlevel
    return None


def exports_of(path: str) -> set[str]:
    pe = pefile.PE(path, fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_EXPORT"]])
    names = set()
    if hasattr(pe, "DIRECTORY_ENTRY_EXPORT"):
        for sym in pe.DIRECTORY_ENTRY_EXPORT.symbols:
            if sym.name:
                names.add(sym.name.decode("ascii", "ignore"))
    pe.close()
    return names


def check(path: str) -> None:
    exe_dir = os.path.dirname(os.path.abspath(path))
    pe = pefile.PE(path, fast_load=True)
    pe.parse_data_directories(
        directories=[
            pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"],
            pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT"],
        ]
    )
    imports = list(getattr(pe, "DIRECTORY_ENTRY_IMPORT", []))
    imports += list(getattr(pe, "DIRECTORY_ENTRY_DELAY_IMPORT", []))
    missing_total = 0
    for entry in imports:
        dll_name = entry.dll.decode("ascii", "ignore")
        dll_path = find_dll(dll_name, exe_dir)
        if dll_path is None:
            print(f"[UNRESOLVED DLL] {dll_name}")
            missing_total += 1
            continue
        exports = exports_of(dll_path)
        missing = []
        for imp in entry.imports:
            if imp.name is None:
                continue  # ordinal import, skip
            name = imp.name.decode("ascii", "ignore")
            if name not in exports:
                missing.append(name)
        if missing:
            missing_total += len(missing)
            print(f"[MISSING in {dll_name}] ({dll_path})")
            for name in missing:
                print(f"    {name}")
    if missing_total == 0:
        print("all named imports resolved")
    pe.close()


if __name__ == "__main__":
    check(sys.argv[1])
