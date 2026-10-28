"""LEAPP modules excluded from Cultivator's managed-Python runtime.

Native extensions packaged inside a Python wheel are supported. Separately
installed DLLs, executables and Git-based dependency chains are not.
"""
import re
from pathlib import PurePosixPath

EXCLUDED_MODULES = {
    "android": {
        "scripts/artifacts/Threema.py": "SQLCipher native library/DLL dependency",
        "scripts/artifacts/hldPrivacySafe.py": "SQLCipher native library/DLL dependency",
        "scripts/artifacts/weChat.py": "SQLCipher native library/DLL dependency",
        "scripts/artifacts/browserArtifactsViaMisterSkinnylegs.py": "Git-installed browser dependency chain",
        "scripts/artifacts/tutanota.py": "Chromium reader supplied by the Git-installed browser dependency chain",
    },
    "ios": {
        "scripts/artifacts/logarchive.py": "Requires the external unifiedlog_iterator executable",
    },
    "vehicle": {},
}

EXCLUDED_FILES = {
    "android": (),
    "ios": (
        "admin/scripts/fetch_unifiedlog_iterator.py",
        "scripts/unifiedlogs.py",
        "scripts/log_filter.py",
        "bin/PROVENANCE.md",
        "bin/THIRD-PARTY-NOTICES-unifiedlog_iterator.txt",
    ),
    "vehicle": (),
}

EXCLUDED_REQUIREMENTS = {
    "android": ("sqlcipher3", "mister-skinnylegs", "lz4", "zstd", "brotli"),
    "ios": ("pyahocorasick", "certifi"),
    "vehicle": (),
}


def requirement_name(line):
    name = PurePosixPath(line).name.split("-", 1)[0] if line.startswith("whl_files/") else re.split(r"[\s<>=!~;\[]", line, 1)[0]
    return re.sub(r"[-_.]+", "-", name).lower()


def include_policy_file(relative, platform):
    relative = PurePosixPath(relative).as_posix()
    if relative in EXCLUDED_MODULES[platform] or relative in EXCLUDED_FILES[platform]:
        return False
    if relative.startswith("whl_files/") and requirement_name(relative) in EXCLUDED_REQUIREMENTS[platform]:
        return False
    # Never vendor standalone native binaries or a previously downloaded reader.
    return not (relative.startswith(("bin/", "native-tools/")) or PurePosixPath(relative).suffix.lower() in (".dll", ".exe", ".so", ".dylib"))


def filter_requirements(text, platform):
    requirements = []
    for line in text.splitlines():
        line = line.split(" #", 1)[0].strip()
        if not line or line.startswith("#") or requirement_name(line) in EXCLUDED_REQUIREMENTS[platform]:
            continue
        if "git+" in line.lower():
            raise ValueError(f"External Git dependency is unsupported: {line}")
        requirements.append(line)
    return "# Cultivator managed-Python LEAPP dependencies; external tools are excluded.\n" + "\n".join(requirements) + "\n"
