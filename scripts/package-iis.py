#!/usr/bin/env python3
"""Make a private, ready-configured IIS ZIP from the Windows release executable."""
import argparse
import hashlib
import os
import pathlib
import re
import secrets
import struct
import subprocess
import urllib.parse
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=pathlib.Path, required=True)
    parser.add_argument('--output', type=pathlib.Path, required=True)
    parser.add_argument('--origin', default='https://ai.example.com')
    args = parser.parse_args()
    origin = args.origin.rstrip('/')
    url = urllib.parse.urlsplit(origin)
    if (url.scheme != 'https' or not url.hostname or url.username or url.password
            or url.path or url.query or url.fragment
            or not re.fullmatch(r'https://[A-Za-z0-9.:-]+', origin)):
        parser.error('--origin must be an HTTPS origin, for example https://ai.example.com')
    # Fail early if the input is not an AMD64 Windows PE executable.
    binary = args.binary.read_bytes()
    if binary[:2] != b'MZ':
        parser.error('--binary must be the Windows x64 executable')
    pe = struct.unpack_from('<I', binary, 0x3C)[0]
    if binary[pe:pe + 4] != b'PE\0\0' or struct.unpack_from('<H', binary, pe + 4)[0] != 0x8664:
        parser.error('--binary must be the Windows x64 executable')
    version = re.search(r'^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(), re.M)[1]
    if os.name == 'nt':
        result = subprocess.run([str(args.binary.resolve()), '--version'],
                                check=True, capture_output=True, text=True)
        if result.stdout.strip() != f'go-router {version}':
            parser.error('executable version does not match Cargo.toml')
    env = (ROOT / '.env.example').read_text()
    env = re.sub(r'\A.*?(?=# --- Required)',
                 '# Ready for IIS. Edit PUBLIC_URL only if using a different hostname.\n'
                 '# The random admin password below belongs to this ZIP; keep it private.\n\n', env, flags=re.S)
    env = re.sub(r'^ADMIN_PASSWORD=.*$', 'ADMIN_PASSWORD=' + secrets.token_urlsafe(32), env, flags=re.M)
    env = re.sub(r'^PUBLIC_URL=.*$', 'PUBLIC_URL=' + origin, env, flags=re.M)
    env = re.sub(r'^COOKIE_SECURE=.*$', 'COOKIE_SECURE=true', env, flags=re.M)
    readme = (ROOT / 'iis/README.txt').read_text().replace('@ORIGIN@', origin).replace('@HOST@', url.hostname)
    checksum = hashlib.sha256(binary).hexdigest()
    source_ref = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, check=True,
                                capture_output=True, text=True).stdout.strip()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(args.output, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        def add(name, content):
            archive.writestr('go-router/' + name, content)
        add('go-router.exe', binary)
        add('.env', env.replace('\n', '\r\n'))
        add('README.txt', readme.replace('\n', '\r\n'))
        add('LICENSE', (ROOT / 'LICENSE').read_bytes())
        add('config/models.json', (ROOT / 'config/models.json').read_bytes())
        add('iis/web.config', (ROOT / 'iis/web.config').read_bytes())
        add('data/', b'')
        add('data/logs/', b'')
        add('PACKAGE-INFO.txt', f'Go Router {version}\r\nWindows x64; static C runtime; bundled SQLite.\r\n'
            f'Executable SHA256: {checksum}\r\n'
            f'Source: https://github.com/AhmedDatori/openrouter-from-opencode/tree/{source_ref}\r\n'
            'Includes a new random admin password and no upstream API keys or existing database.\r\n')
    digest = hashlib.sha256(args.output.read_bytes()).hexdigest()
    args.output.with_suffix(args.output.suffix + '.sha256').write_text(
        f'{digest}  {args.output.name}\n', encoding='ascii')
    print(f'Created {args.output.name} ({args.output.stat().st_size:,} bytes). Credentials are inside .env.')


if __name__ == '__main__':
    main()
