"""Compare native array-growth workloads; compiler time is excluded.

Example: py tools/bench_array_growth.py --compiler old=path/to/old.exe
         --compiler current=target/release/aegis-cli.exe
Output includes raw samples, medians, source/EXE hashes and validated checksums.
Whole-process time includes Windows startup, allocation, indexing and cleanup.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time


def run(command):
    result = subprocess.run(command, capture_output=True, timeout=30,
                            creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
    if result.returncode:
        raise RuntimeError(f'{command[0]} exited {result.returncode}: {result.stderr!r}')
    return result


def benchmark(compiler, count, runs, folder):
    source = f'''fn main() {{
        let a: [i64] = [];
        let i = 0;
        while (i < {count}) {{ a = push(a, i); i = i + 1; }}
        let sum = 0;
        i = 0;
        while (i < len(a)) {{ sum = sum + a[i]; i = i + 1; }}
        printi(len(a)); printi(a[{count - 1}]); printi(sum);
    }}'''
    src, exe = folder/'growth.ae', folder/'growth.exe'
    src.write_text(source, encoding='utf-8')
    run([str(compiler), 'build', str(src), str(exe)])
    expected = f'{count}\n{count - 1}\n{count * (count - 1) // 2}\n'.encode()
    samples = []
    for index in range(runs + 1):
        started = time.perf_counter_ns()
        result = run([str(exe)])
        elapsed = time.perf_counter_ns() - started
        if result.stdout != expected or result.stderr:
            raise RuntimeError(f'native checksum mismatch: {result.stdout!r}, {result.stderr!r}')
        if index:  # Exclude the first process as warmup.
            samples.append(elapsed)
    return {'elements': count, 'samples_ns': samples,
            'median_ns': statistics.median(samples),
            'source_sha256': hashlib.sha256(source.encode()).hexdigest(),
            'native_sha256': hashlib.sha256(exe.read_bytes()).hexdigest(),
            'verified_sum': count * (count - 1) // 2}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--compiler', action='append', required=True, metavar='LABEL=PATH')
    parser.add_argument('--sizes', type=int, nargs='+', default=[2048, 8192, 16384])
    parser.add_argument('--runs', type=int, default=5)
    args = parser.parse_args()
    if not 1 <= args.runs <= 11 or any(not 1 <= n <= 1_048_576 for n in args.sizes):
        parser.error('runs must be 1..11; sizes must be 1..1048576')
    report = {'platform': platform.platform(), 'machine': platform.machine(),
              'timing': 'whole native process; one warmup excluded; compile time excluded',
              'compilers': {}}
    with tempfile.TemporaryDirectory(prefix='aegis-growth-') as temp:
        folder = Path(temp)
        for argument in args.compiler:
            label, separator, value = argument.partition('=')
            if not separator or not label or label in report['compilers']:
                parser.error('compiler must be a unique LABEL=PATH')
            compiler = Path(value).resolve(strict=True)
            report['compilers'][label] = {
                'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
                'results': [benchmark(compiler, n, args.runs, folder) for n in args.sizes],
            }
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
