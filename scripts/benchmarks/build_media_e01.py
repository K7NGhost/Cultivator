"""Copy a corpus, populate a filesystem, and encode/verify an E01 with libewf.

Uses installed independent writers, not Inodex, to prepare benchmark evidence.
Never overwrites an existing output directory or modifies the source corpus.
"""
import argparse
import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import time


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def write_fat16(copied, raw, manifest):
    """Specification-built FAT16/VFAT whole volume, validated separately with 7-Zip.

    This benchmark corpus is flat. Short aliases are unique; VFAT entries preserve
    all original filenames. Payloads occupy ordinary contiguous FAT chains.
    """
    total_sectors, cluster_sectors, fat_sectors, root_entries = 786432, 32, 128, 8192
    data_sector = 1 + 2 * fat_sectors + root_entries * 32 // 512
    cluster_bytes = cluster_sectors * 512
    boot = bytearray(512)
    boot[:11] = b"\xeb\x3c\x90MSDOS5.0"
    struct.pack_into("<HBHBHHBHHHII", boot, 11, 512, cluster_sectors, 1, 2, root_entries, 0, 0xf8, fat_sectors, 63, 255, 0, total_sectors)
    boot[36:39] = b"\x80\x00\x29"
    struct.pack_into("<I", boot, 39, 0xc0172026)
    boot[43:54], boot[54:62], boot[510:512] = b"CULT_MEDIA ", b"FAT16   ", b"\x55\xaa"
    fat = bytearray(fat_sectors * 512)
    struct.pack_into("<HH", fat, 0, 0xfff8, 0xffff)
    directory = bytearray(root_entries * 32)
    directory[:11], directory[11] = b"CULT_MEDIA ", 8
    slot, cluster = 1, 2
    with raw.open("xb") as output:
        output.truncate(total_sectors * 512)
        output.write(boot)
        for number, record in enumerate(manifest, 1):
            relative = Path(record["path"])
            if len(relative.parts) != 1:
                raise ValueError("FAT16 benchmark builder expects a flat corpus")
            name = relative.name
            suffix = "".join(c for c in relative.suffix[1:].upper() if c.isascii() and c.isalnum())[:3]
            short = (f"F{number:07d}" + suffix.ljust(3)).encode("ascii")
            checksum = 0
            for byte in short:
                checksum = (((checksum & 1) << 7) + (checksum >> 1) + byte) & 255
            units = name.encode("utf-16le") + b"\x00\x00"
            units += b"\xff\xff" * ((-len(units) // 2) % 13)
            pieces = len(units) // 26
            for sequence in range(pieces, 0, -1):
                entry = bytearray(32)
                entry[0], entry[11], entry[13] = sequence | (0x40 if sequence == pieces else 0), 15, checksum
                part = units[(sequence - 1) * 26:sequence * 26]
                entry[1:11], entry[14:26], entry[28:32] = part[:10], part[10:22], part[22:]
                directory[slot * 32:(slot + 1) * 32] = entry
                slot += 1
            count = (record["bytes"] + cluster_bytes - 1) // cluster_bytes
            if cluster + count > (total_sectors - data_sector) // cluster_sectors + 2:
                raise ValueError("Corpus exceeds FAT16 benchmark volume")
            for current in range(cluster, cluster + count):
                struct.pack_into("<H", fat, current * 2, current + 1 if current + 1 < cluster + count else 0xffff)
            entry = bytearray(32)
            entry[:11], entry[11] = short, 32
            date = ((2026 - 1980) << 9) | (10 << 5) | 23
            for offset in [16, 18, 24]:
                struct.pack_into("<H", entry, offset, date)
            for offset in [14, 22]:
                struct.pack_into("<H", entry, offset, 12 << 11)
            struct.pack_into("<HI", entry, 26, cluster if count else 0, record["bytes"])
            directory[slot * 32:(slot + 1) * 32] = entry
            slot += 1
            if slot >= root_entries:
                raise ValueError("Corpus exceeds FAT16 root directory")
            output.seek(data_sector * 512 + (cluster - 2) * cluster_bytes)
            with (copied / relative).open("rb") as input_file:
                shutil.copyfileobj(input_file, output)
            cluster += count
        output.seek(512)
        output.write(fat)
        output.write(fat)
        output.write(directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mke2fs", type=Path, required=True)
    parser.add_argument("--libewf", type=Path, required=True)
    parser.add_argument("--libewf-helper", type=Path, required=True)
    parser.add_argument("--resume", action="store_true", help="Reuse and reverify an existing copied corpus")
    parser.add_argument("--filesystem", choices=["fat16", "ext2"], default="fat16")
    parser.add_argument("--reuse-raw", action="store_true", help="Reuse an already completed raw volume after external validation")
    args = parser.parse_args()
    source = args.source.resolve(strict=True)
    output = args.output.resolve()
    if output.is_relative_to(source):
        raise ValueError("Output must be outside the original images")
    output.mkdir(parents=True, exist_ok=args.resume)
    copied = output / "manual_testimages-copy"
    start = time.perf_counter()
    files = sorted(path for path in source.rglob("*") if path.is_file())
    if any(path.is_symlink() for path in source.rglob("*")):
        raise ValueError("Refusing a corpus with symbolic links")
    manifest = []
    for original in files:
        relative = original.relative_to(source)
        destination = copied / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        before = original.stat()
        sha256 = digest(original)
        if not (args.resume and destination.is_file()):
            shutil.copy2(original, destination)
        after = original.stat()
        if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise ValueError(f"Source changed during copying: {original}")
        if digest(destination) != sha256:
            raise ValueError(f"Copy differs: {relative}")
        manifest.append({"path": relative.as_posix(), "bytes": before.st_size, "sha256": sha256})
    print(f"COPY files={len(files)} bytes={sum(f['bytes'] for f in manifest)} seconds={time.perf_counter()-start:.3f}", flush=True)
    (output / "corpus-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    raw = output / f"manual-testimages.{args.filesystem}.raw"
    # At least 384 MiB, rounded to a 4 KiB block boundary, leaving room for metadata.
    size = max(384 * 1024 * 1024, sum(f["bytes"] for f in manifest) * 3 // 2)
    blocks = (size + 4095) // 4096
    environment = os.environ.copy()
    config = args.mke2fs.with_suffix(".conf")
    if config.is_file():
        environment["MKE2FS_CONFIG"] = str(config)
    command = [str(args.mke2fs), "-F", "-t", "ext2", "-b", "4096", "-I", "256", "-m", "0",
               "-L", "CULT_MEDIA", "-d", str(copied), str(raw), str(blocks)]
    if args.reuse_raw:
        if not args.resume or not raw.is_file():
            raise ValueError("--reuse-raw requires --resume and a completed existing volume")
        command = ["Reuse the externally validated existing raw filesystem"]
    elif args.filesystem == "ext2":
        if raw.exists():
            raise FileExistsError(raw)
        subprocess.run(command, env=environment, check=True)
    else:
        write_fat16(copied, raw, manifest)
        command = ["Python standard-library FAT16/VFAT writer in this script"]
    spec = importlib.util.spec_from_file_location("benchmark_libewf", args.libewf_helper)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    library = module.Libewf(args.libewf)
    version = library.lib.libewf_get_version().decode()
    print(f"WRITER libewf={version} raw_bytes={raw.stat().st_size}", flush=True)
    basename = output / "manual-testimages"
    if list(output.glob("manual-testimages.E[0-9][0-9]")):
        raise FileExistsError("Refusing to overwrite E01 segments")
    start = time.perf_counter()
    library.write(basename, raw.read_bytes(), 6, 2)  # EnCase 6 profile, best compression.
    segments = sorted(output.glob("manual-testimages.E[0-9][0-9]"))
    handle = library.handle(segments, 1)
    decoded = hashlib.sha256()
    try:
        buffer = ctypes.create_string_buffer(1024 * 1024)
        for offset in range(0, raw.stat().st_size, len(buffer)):
            count = min(len(buffer), raw.stat().st_size - offset)
            actual = library.call("read_random", handle, buffer, count, offset)
            if actual != count:
                raise ValueError("Short read from independently written E01")
            decoded.update(buffer.raw[:count])
    finally:
        library.finish(handle)
    raw_sha256 = digest(raw)
    if decoded.hexdigest() != raw_sha256:
        raise ValueError("Decoded E01 does not match the original raw filesystem")
    # Recheck originals and copied inputs after all preparation.
    for record in manifest:
        relative = Path(record["path"])
        if digest(source / relative) != record["sha256"] or digest(copied / relative) != record["sha256"]:
            raise ValueError(f"Corpus changed: {relative}")
    result = {"source": str(source), "copy": str(copied), "files": len(manifest),
              "bytes": sum(f["bytes"] for f in manifest), "filesystem": "FAT16/VFAT whole volume, 16384-byte clusters" if args.filesystem == "fat16" else "ext2 whole volume",
              "rawBytes": raw.stat().st_size, "rawSha256": raw_sha256,
              "libewfVersion": version, "format": "EnCase 6 profile", "compression": 2,
              "segments": [{"path": str(p), "bytes": p.stat().st_size, "sha256": digest(p)} for p in segments],
              "decodedSha256MatchesRaw": True, "sourceAndCopyHashesVerified": True,
              "mke2fsCommand": command}
    (output / "image-provenance.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(f"E01 verified segments={len(segments)} bytes={sum(p.stat().st_size for p in segments)} write_verify_seconds={time.perf_counter()-start:.3f}", flush=True)


if __name__ == "__main__":
    main()
