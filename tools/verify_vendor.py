"""Detect accidental changes to the reviewed third-party source snapshot."""
from pathlib import Path
import hashlib

root = Path(__file__).resolve().parents[1]/'third_party'/'iced-x86'
manifest = root.parent/'iced-x86.sha256'
expected = set()
def require(condition, message):
    if not condition:
        raise SystemExit('vendor verification failed: '+message)

for line in manifest.read_text(encoding='utf-8').splitlines():
    digest, name = line.split('  ', 1)
    path = root/name
    require(len(digest) == 64 and not Path(name).is_absolute() and '..' not in Path(name).parts, 'invalid manifest entry')
    require(path.resolve().is_relative_to(root.resolve()) and name not in expected, 'invalid/duplicate path')
    expected.add(name)
    require(hashlib.sha256(path.read_bytes()).hexdigest() == digest, f'source changed: {name}')
actual = {p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file()}
require(expected == actual, 'source snapshot has missing/extra files')
print(f'Verified {len(expected)} vendored iced-x86 files')
