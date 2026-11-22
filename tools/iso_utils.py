#!/usr/bin/env python3
"""Small ISO and initramfs helpers used by the community Builder."""
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
from pathlib import Path, PurePosixPath
import re
import subprocess

LIMIT = 100_000_000
MAX_ROOTFS = 512 * 1024 * 1024
PROFILE = 'etc/bunbun/wifi.conf'


class ConfigError(Exception):
    pass


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def unpack_cpio(data):
    entries, names, offset = [], set(), 0
    while True:
        header = data[offset:offset + 110]
        if len(header) != 110 or header[:6] != b'070701':
            raise ConfigError('Unsupported or truncated newc initramfs.')
        try:
            fields = [int(header[6+i*8:14+i*8], 16) for i in range(13)]
        except ValueError:
            raise ConfigError('Invalid newc header.') from None
        size, namesize = fields[6], fields[11]
        offset += 110
        if not 1 <= namesize <= 4096 or offset + namesize > len(data) or data[offset+namesize-1] != 0:
            raise ConfigError('Invalid newc name.')
        raw_name = data[offset:offset + namesize - 1]
        try:
            name = raw_name.decode('utf-8').removeprefix('./')
        except UnicodeError:
            raise ConfigError('Unsupported archive filename.') from None
        offset = (offset + namesize + 3) & ~3
        if name == 'TRAILER!!!':
            if any(data[offset:]):
                raise ConfigError('Multiple archives or trailing payload are unsupported.')
            return entries
        path = PurePosixPath(name)
        if path.is_absolute() or '..' in path.parts or '\0' in name or name in names:
            raise ConfigError('Unsafe or duplicate archive path.')
        if offset + size > len(data):
            raise ConfigError('Truncated archive file.')
        names.add(name)
        entries.append([name, fields, data[offset:offset + size]])
        if len(entries) > 100000:
            raise ConfigError('Too many archive entries.')
        offset = (offset + size + 3) & ~3


def pack_cpio(entries):
    result = bytearray()
    for name, original, payload in [*entries, ['TRAILER!!!', [0]*13, b'']]:
        fields = original.copy()
        encoded = name.encode('utf-8') + b'\0'
        fields[6], fields[11], fields[12] = len(payload), len(encoded), 0
        result.extend(b'070701' + ''.join(f'{n:08x}' for n in fields).encode('ascii'))
        result.extend(encoded)
        result.extend(b'\0' * (-len(result) % 4))
        result.extend(payload)
        result.extend(b'\0' * (-len(result) % 4))
    result.extend(b'\0' * (-len(result) % 512))
    return bytes(result)


def xorriso_argument(value):
    if os.name == 'nt' and isinstance(value, Path):
        drive = value.drive
        if not re.fullmatch(r'[A-Za-z]:', drive) or not value.is_absolute():
            raise ConfigError('Windows xorriso requires absolute paths on local drives.')
        return f'/{drive[0].lower()}/' + '/'.join(value.parts[1:])
    return str(value)


def run_xorriso(binary, arguments):
    result = subprocess.run([binary, '-no_rc', *map(xorriso_argument, arguments)],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=300)
    if result.returncode:
        raise ConfigError('xorriso operation failed; check file access and ISO format.')
    return (result.stdout + result.stderr).decode('utf-8', errors='replace')


def extract(binary, image, name, destination):
    run_xorriso(binary, ['-osirrox', 'on', '-indev', image, '-extract', name, destination])
    if not destination.is_file() or destination.is_symlink():
        raise ConfigError('Required boot file could not be read.')
    destination.chmod(0o600)


def ensure_absent(paths):
    if any(p.exists() or p.is_symlink() for p in paths):
        raise ConfigError('Output or sidecar already exists; choose a new output filename.')
