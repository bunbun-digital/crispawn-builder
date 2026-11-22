#!/usr/bin/env python3
"""Replace the JSON settings inside a NEW CRISPAWN ISO.

Copyright (c) 2026 bunbun-digital and contributors. SPDX-License-Identifier: Apache-2.0
"""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import lzma
import os
from pathlib import Path
import re
import stat
import sys
import tempfile

spec = importlib.util.spec_from_file_location("crispawn_iso", Path(__file__).with_name("iso_utils.py"))
iso = importlib.util.module_from_spec(spec)
spec.loader.exec_module(iso)
CONFIG = "etc/perfect-backup/live.json"
CAPABILITIES = "etc/perfect-backup/release-capabilities.json"
CREDENTIALS = "etc/crispawn/credentials.json"


class UnsupportedRelease(ValueError):
    pass


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate configuration key")
        result[key] = value
    return result


def validate_config(data):
    if not 1 <= len(data) <= 65536:
        raise ValueError("configuration must be 1..65536 bytes")
    value = json.loads(data, object_pairs_hook=unique_object,
                       parse_constant=lambda _: (_ for _ in ()).throw(ValueError("nonfinite number")))
    if not isinstance(value, dict) or type(value.get("schema_version", 1)) is not int or value.get("schema_version", 1) != 1:
        raise ValueError("expected configuration schema 1")
    allowed = {"schema_version", "operation", "source_id", "source_ids", "language", "backup_directory", "restore", "tuning", "gui",
               "menu_seconds", "completion_seconds", "poweroff_on_success", "log_policy", "log_sync_interval_ms", "probe_interval_ms"}
    if value.keys() - allowed:
        raise ValueError("unknown configuration field")
    if value.get("language", "ja") not in ("ja", "en"):
        raise ValueError("invalid language")
    source_ids = value.get("source_ids", [])
    if not isinstance(source_ids, list) or len(source_ids) > 64 or any(not isinstance(item, str) for item in source_ids):
        raise ValueError("source_ids must contain at most 64 strings")
    if value.get("source_id") is not None and source_ids:
        raise ValueError("source_id and source_ids are mutually exclusive")
    if value.get("log_policy", "always") not in ("always", "on_error", "never"):
        raise ValueError("invalid log_policy")
    interval = value.get("log_sync_interval_ms", 0)
    if type(interval) is not int or not 0 <= interval <= 60000:
        raise ValueError("log_sync_interval_ms must be 0..60000")
    return data


def read_initramfs(compressed):
    decoder = lzma.LZMADecompressor(format=lzma.FORMAT_XZ, memlimit=256 * 1024**2)
    raw = decoder.decompress(compressed, max_length=iso.MAX_ROOTFS + 1)
    if len(raw) > iso.MAX_ROOTFS or not decoder.eof or decoder.unused_data:
        raise ValueError("unsupported or oversized initramfs")
    return iso.unpack_cpio(raw)


def read_release_capabilities(index):
    entry = index.get(CAPABILITIES)
    if entry is None:
        return [1], None
    fields, raw = entry
    if not stat.S_ISREG(fields[1]) or fields[4] != 1 or not 1 <= len(raw) <= 65536:
        raise UnsupportedRelease("invalid release capability file")
    value = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)
    if (not isinstance(value, dict) or set(value) != {"kind", "schema_version", "release_id", "architecture",
                                                    "kernel_build", "live_config_versions", "capabilities"}
            or value["kind"] != "crispawn.release_capabilities"
            or type(value["schema_version"]) is not int or value["schema_version"] != 1
            or value["architecture"] != "x86_64"):
        raise UnsupportedRelease("invalid release capability document")
    for name in ("release_id", "kernel_build"):
        if not isinstance(value[name], str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,127}", value[name]):
            raise UnsupportedRelease("invalid release identity")
    versions = value["live_config_versions"]
    caps = value["capabilities"]
    if (not isinstance(versions, list) or not 1 <= len(versions) <= 16
            or any(type(v) is not int or not 1 <= v <= 65535 for v in versions)
            or len(set(versions)) != len(versions)
            or not isinstance(caps, list) or not 1 <= len(caps) <= 128
            or any(not isinstance(c, str) or not re.fullmatch(r"[a-z][a-z0-9_.-]{1,95}", c) for c in caps)
            or len(set(caps)) != len(caps)):
        raise UnsupportedRelease("invalid release version or capabilities")
    return versions, set(caps)


def required_capabilities(runtime):
    caps = {"sources." + runtime["sources"]["mode"], "startup." + runtime["startup"]["mode"],
            "destination." + runtime["destination"]["kind"], "completion." + runtime["completion"]["action"],
            "ui." + runtime["ui"]["mode"], "ui.lang." + runtime["ui"]["language"],
            "logging." + runtime["logging"]["policy"], "processing.auto",
            "processing.profile." + runtime["processing"]["initial_profile"]}
    if runtime.get("network") != {"mode": "none"}:
        raise ValueError("community Builder supports only local USB backups")
    return caps


def encode_document(value):
    data = (json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode("utf-8")
    if len(data) > 65536:
        raise ValueError("settings exceed the runtime document size limit")
    return data


def select_settings(config, index, payload):
    versions, caps = read_release_capabilities(index)
    if 2 not in versions and CONFIG in index:
        original = json.loads(index[CONFIG][1], object_pairs_hook=unique_object)
        if (not isinstance(original, dict) or original.get("kind") == "crispawn.live"
                or type(original.get("schema_version", 1)) is not int or original.get("schema_version", 1) != 1):
            raise UnsupportedRelease("base runtime does not match its release declaration")
    if payload is None:
        if 2 in versions or 1 not in versions:
            raise UnsupportedRelease("new runtime requires Builder settings")
        return config, {}
    runtime, credentials = payload["runtime"], payload["credentials"]
    if runtime["kind"] != "crispawn.live" or runtime["schema_version"] != 2:
        raise ValueError("invalid Builder runtime document")
    if runtime.get("network") != {"mode": "none"} or credentials is not None:
        raise ValueError("community Builder supports only local USB backups")
    if caps is not None and not required_capabilities(runtime) <= caps:
        raise UnsupportedRelease("release is missing a required capability")
    updates = {path: None for path in (CREDENTIALS, iso.PROFILE) if path in index}
    if 2 not in versions:
        if 1 not in versions:
            raise UnsupportedRelease("requested settings require runtime v2")
        return config, updates

    return encode_document(runtime), updates


def update_initramfs(compressed, config):
    validate_config(config)
    return assemble_initramfs(read_initramfs(compressed), config, {})


def assemble_initramfs(entries, config, updates):
    index = {name: (fields, data) for name, fields, data in entries}
    marker = index.get("etc/perfect-backup/live-root")
    if not marker or not stat.S_ISREG(marker[0][1]) or marker[1] != b"perfect-backup-liveos-v1\n":
        raise ValueError("not a perfect backup LiveOS")
    for parent in ("etc", "etc/perfect-backup"):
        if parent not in index or not stat.S_ISDIR(index[parent][0][1]):
            raise ValueError("invalid settings directory")
    if CONFIG in index and (not stat.S_ISREG(index[CONFIG][0][1]) or index[CONFIG][0][4] != 1):
        raise ValueError("settings must be a regular unlinked file")
    inode = max(fields[0] for _, fields, _ in entries)
    for path, update in {CONFIG: (0o600, config), **updates}.items():
        if (not stat.S_ISDIR(index.get(path.rsplit("/", 1)[0], ([0, 0], None))[0][1])
                or path in index and
                (not stat.S_ISREG(index[path][0][1]) or index[path][0][4] != 1)):
            raise ValueError("unsafe configuration path in the base ISO")
        entries = [entry for entry in entries if entry[0] != path]
        if update is not None:
            mode, data = update
            inode += 1
            entries.append([path, [inode, stat.S_IFREG | mode, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0], data])
    packed = iso.pack_cpio(entries)
    if len(packed) > iso.MAX_ROOTFS:
        raise ValueError("configured initramfs exceeds its size limit")
    return lzma.compress(packed, preset=9, check=lzma.CHECK_CRC32)


def boot_entries(binary, image):
    report = iso.run_xorriso(binary, ["-indev", image, "-report_el_torito", "plain"])
    return re.findall(r"El Torito boot img\s*:\s*\d+\s+(BIOS|UEFI)\s+y\s+none\s", report)


def configure(source, output, config, binary, payload=None):
    validate_config(config)
    source, output = source.resolve(strict=True), output.absolute()
    sidecars = [Path(str(output) + suffix) for suffix in (".sha256", ".json")]
    iso.ensure_absent([output, *sidecars])
    if not source.is_file() or not 0 < source.stat().st_size < iso.LIMIT or not output.parent.is_dir():
        raise ValueError("input ISO or output directory is invalid")
    original = iso.digest(source)
    platforms = boot_entries(binary, source)
    with tempfile.TemporaryDirectory(prefix=".perfect-backup-config-", dir=output.parent) as temporary:
        work = Path(temporary)
        iso.extract(binary, source, "/boot/initrd", work / "initrd")
        kernel = work / "before-0"
        iso.extract(binary, source, "/boot/bzImage", kernel)
        entries = read_initramfs((work / "initrd").read_bytes())
        index = {name: (fields, data) for name, fields, data in entries}
        config, updates = select_settings(config, index, payload)
        updated = work / "updated-initrd"
        updated.write_bytes(assemble_initramfs(entries, config, updates))
        image = work / "configured.iso"
        iso.run_xorriso(binary, ["-indev", source, "-outdev", image, "-boot_image", "any", "replay",
                                "-overwrite", "on", "-map", updated, "/boot/initrd", "-commit", "-end"])
        image.chmod(0o600)
        if not 0 < image.stat().st_size < iso.LIMIT or boot_entries(binary, image) != platforms:
            raise ValueError("ISO size or boot entries changed unexpectedly")
        iso.extract(binary, image, "/boot/initrd", work / "check-initrd")
        if iso.digest(updated) != iso.digest(work / "check-initrd"):
            raise ValueError("settings readback mismatch")
        readback = {name: (fields, data) for name, fields, data in
                    read_initramfs((work / "check-initrd").read_bytes())}
        if CONFIG not in readback or readback[CONFIG][1] != config:
            raise ValueError("embedded live.json differs from requested configuration")
        fields = readback[CONFIG][0]
        if fields[1:5] != [stat.S_IFREG | 0o600, 0, 0, 1]:
            raise ValueError("unexpected embedded live.json permissions or ownership")
        for name, update in updates.items():
            if update is None:
                if name in readback:
                    raise ValueError("network profile was not removed")
            else:
                mode, content = update
                if (name not in readback or readback[name][1] != content
                        or readback[name][0][1:5] != [stat.S_IFREG | mode, 0, 0, 1]):
                    raise ValueError("network configuration readback mismatch")
        for number, name in enumerate(["/boot/bzImage"] + (["/boot/fat.efi"] if "UEFI" in platforms else [])):
            before, after = work / f"before-{number}", work / f"after-{number}"
            if number != 0:
                iso.extract(binary, source, name, before)
            iso.extract(binary, image, name, after)
            if iso.digest(before) != iso.digest(after):
                raise ValueError("signed boot component changed")
        if iso.digest(source) != original:
            raise ValueError("input ISO changed during configuration")
        metadata = {"schema_version": 1, "kind": "perfect-backup-configuration",
                    "created_at": datetime.now(timezone.utc).isoformat(), "parent_iso_sha256": original,
                    "iso_sha256": iso.digest(image), "iso_size_bytes": image.stat().st_size,
                    "configuration_sha256": hashlib.sha256(config).hexdigest(), "configuration_path": "/" + CONFIG,
                    "configuration_schema_version": json.loads(config).get("schema_version", 1),
                    "boot_entries": platforms, "kernel_preserved": True, "boot_tested": False}
        legacy_wifi = readback.get(iso.PROFILE, (None, b""))[1]
        metadata["contains_network_credentials"] = CREDENTIALS in readback or any(
            key in legacy_wifi for key in (b"psk=", b"sae_password=")
        )
        published = []
        try:
            for path, data in [(sidecars[0], f'{metadata["iso_sha256"]}  {output.name}\n'),
                               (sidecars[1], json.dumps(metadata, indent=2) + "\n")]:
                with path.open("x") as stream:
                    published.append(path)
                    stream.write(data)
                path.chmod(0o600)
            os.link(image, output)
        except OSError:
            for path in published:
                path.unlink()
            raise
        return metadata


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_iso", type=Path)
    parser.add_argument("output_iso", type=Path)
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--xorriso", default="xorriso")
    parser.add_argument("--runtime-v2-stdin", action="store_true")
    args = parser.parse_args()
    payload = None
    if args.runtime_v2_stdin:
        raw = sys.stdin.buffer.read(131073)
        if not 0 < len(raw) <= 131072:
            raise ValueError("invalid Builder runtime payload size")
        payload = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)
        if not isinstance(payload, dict) or set(payload) != {"runtime", "credentials"}:
            raise ValueError("invalid Builder runtime payload")
    metadata = configure(args.input_iso, args.output_iso, args.config.read_bytes(), args.xorriso,
                         payload)
    print(f'ISO_CONFIGURED size_bytes={metadata["iso_size_bytes"]} settings={metadata["configuration_path"]}')


if __name__ == "__main__":
    try:
        main()
    except UnsupportedRelease:
        print("The selected base ISO does not declare support for the requested settings.", file=sys.stderr)
        raise SystemExit(3)
    except (OSError, ValueError, KeyError, TypeError, lzma.LZMAError, iso.ConfigError):
        print("Backup settings update failed; input ISO and pre-existing outputs were preserved.", file=sys.stderr)
        raise SystemExit(2)
