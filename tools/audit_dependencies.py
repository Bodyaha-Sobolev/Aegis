"""Query public OSV records for exact registry versions in Cargo.lock."""
from pathlib import Path
import json
import tomllib
import urllib.request
import subprocess
import re

root = Path(__file__).resolve().parents[1]
lock = tomllib.loads((root/'Cargo.lock').read_text(encoding='utf-8'))
target = 'x86_64-pc-windows-msvc'
tree = subprocess.run(['cargo','tree','--locked','--workspace','--target',target,'--prefix','none','--format','{p}'],cwd=root,text=True,capture_output=True,check=True).stdout
selected = {m.groups() for line in tree.splitlines() if (m := re.match(r'^(\S+) v(\S+)',line))}
packages = [p for p in lock['package'] if p.get('source','').startswith('registry+') and (p['name'],p['version']) in selected]
queries = [{'package': {'name': p['name'], 'ecosystem': 'crates.io'}, 'version': p['version']} for p in packages]
request = urllib.request.Request('https://api.osv.dev/v1/querybatch',data=json.dumps({'queries':queries}).encode(),headers={'Content-Type':'application/json'})
with urllib.request.urlopen(request,timeout=60) as response:
    results = json.load(response)['results']
findings = 0
informational = 0
for package, result in zip(packages, results, strict=True):
    for vulnerability in result.get('vulns',[]):
        with urllib.request.urlopen('https://api.osv.dev/v1/vulns/'+vulnerability['id'],timeout=30) as response:
            record = json.load(response)
        info = any(a.get('database_specific',{}).get('informational') for a in record.get('affected',[]))
        if info: informational += 1
        else: findings += 1
        print('INFO' if info else 'SECURITY',package['name'],package['version'],vulnerability['id'],record.get('summary',''))
print(f'Checked {len(packages)} locked registry packages for {target}; {findings} security records, {informational} informational records.')
raise SystemExit(bool(findings))
