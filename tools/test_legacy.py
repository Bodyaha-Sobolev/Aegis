"""End-to-end tests: compile AEGIS source, run native PE output, check diagnostics."""
from pathlib import Path
import os
import stat
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
COMPILER = ROOT / 'legacy' / 'asm' / 'aegis.exe'


def run(exe, *args, timeout=10):
    return subprocess.run([str(exe), *map(str, args)], capture_output=True,
                          timeout=timeout, creationflags=subprocess.CREATE_NO_WINDOW)


def main():
    cases = [
        ('literal', 'fn main() { printi(42); return 0; }', b'42\n', 0),
        ('variables', 'fn main() { let value = 41 value = value + 1 printi(value) return 0 }', b'42\n', 0),
        ('precedence', 'fn main() { printi(2+3*4); printi((2+3)*4); printi(20/3); printi(20%3); return 0; }', b'14\n20\n6\n2\n', 0),
        ('signed', 'fn main() { printi(-42); printi(-9223372036854775808); printi(-20/3); printi(-20%3); return 0; }', b'-42\n-9223372036854775808\n-6\n-2\n', 0),
        ('branch', 'fn main() { if (1<2) { printi(11); } else { printi(99); } if (0) { printi(98); } else if (2==2) { printi(22); } else { printi(97); } return 0; }', b'11\n22\n', 0),
        ('loop', 'fn main() { let i=0; let sum=0; while (i<10) { sum=sum+i; i=i+1; } printi(sum); return 0; }', b'45\n', 0),
        ('nested', 'fn main() { let i=0; while(i<3) { let j=0; while(j<2) { printi(i*10+j); j=j+1; } i=i+1; } return 0; }', b'0\n1\n10\n11\n20\n21\n', 0),
        ('calls', 'fn add(a,b) { return a+b; } fn main() { printi(add(20,22)); return 7; }', b'42\n', 7),
        ('forward', 'fn main() { printi(twice(21)); return 0; } fn twice(x) { return x*2; }', b'42\n', 0),
        ('recursion', 'fn fact(n) { if(n<=1) { return 1; } return n*fact(n-1); } fn main() { printi(fact(6)); return 0; }', b'720\n', 0),
        ('nested_calls', 'fn sum(a,b,c,d) { return a+b+c+d; } fn main() { printi(1+sum(1,sum(1,2,3,4),3,4)*2); return 0; }', b'37\n', 0),
        ('compare', 'fn main() { printi(2>=2); printi(2<=1); printi(1!=2); printi(2==2); printi(3>2); printi(!0); printi(!3); return 0; }', b'1\n0\n1\n1\n1\n1\n0\n', 0),
        ('characters', 'fn main() { printc(65); printc(10); exit(9); }', b'A\n', 9),
        ('hex_comments', '\ufeff/* test */ fn main() { // comment\n printi(0x2A); return 0; }', b'42\n', 0),
        ('empty', 'fn main() {}', b'', 0),
    ]
    errors = [
        ('undefined', 'fn main() { printi(missing); }', b'undefined variable'),
        ('no_main', 'fn other() {}', b'fn main() is required'),
        ('unclosed', 'fn main() { return 0;', b'ERROR 1:'),
        ('duplicate_fn', 'fn main() {} fn main() {}', b'function already defined'),
        ('duplicate_var', 'fn main() { let x=1; let x=2; }', b'variable already defined'),
        ('arity', 'fn f(x) { return x; } fn main() { return f(); }', b'wrong number'),
        ('trailing', 'fn main() {} 42', b'unexpected token'),
        ('comment', 'fn main() {} /*', b'unterminated block comment'),
        ('overflow', 'fn main() { return 18446744073709551616; }', b'integer literal'),
        ('scope', 'fn main() { if(1) { let x=42; } printi(x); }', b'undefined variable'),
        ('position', 'fn main() {\n  printi(missing);\n}', b'ERROR 2:10:'),
        ('bad_fn', 'fn main() { return missing(); }', b'undefined function'),
        ('depth', 'fn main() { return ' + '(' * 140 + '1' + ')' * 140 + '; }', b'limit exceeded'),
    ]
    with tempfile.TemporaryDirectory(prefix='aegis-tests-') as folder:
        folder = Path(folder)
        for name, source, expected, code in cases:
            src, exe = folder / (name+'.ae'), folder / (name+'.exe')
            src.write_text(source, encoding='utf-8')
            result = run(COMPILER, 'build', src, exe)
            assert result.returncode == 0, (name, result.stdout, result.stderr, result.returncode)
            result = run(exe)
            assert (result.stdout, result.returncode) == (expected, code), (name, result.stdout, result.stderr, result.returncode)
            print('PASS', name)
        for name, source, diagnostic in errors:
            src, exe = folder / (name+'.ae'), folder / (name+'.exe')
            src.write_text(source, encoding='utf-8')
            result = run(COMPILER, 'build', src, exe)
            assert result.returncode == 2 and diagnostic in result.stdout and not exe.exists(), (name, result.stdout, result.stderr, result.returncode)
            print('PASS reject', name)
        result = run(COMPILER, 'test')
        assert result.returncode == 0 and b'FAIL' not in result.stdout, result.stdout
        print('PASS crypto selftest')
        key, wrong_key = folder/'key.bin', folder/'wrong-key.bin'
        key.write_bytes(os.urandom(32))
        wrong_key.write_bytes(os.urandom(32))
        src, enc, dec, exe = (folder/n for n in ('protected.ae','protected.aeg','restored.ae','protected.exe'))
        src.write_bytes(b'fn main() { printi(123); return 0; }')
        result = run(COMPILER, 'enc', src, enc, '-k', key, '-c', '0xABC', timeout=30)
        assert result.returncode == 0 and enc.exists(), (result.stdout, result.returncode)
        result = run(COMPILER, 'dec', enc, dec, '-k', key, '-c', '0xABC', timeout=30)
        assert result.returncode == 0 and dec.read_bytes() == src.read_bytes(), (result.stdout, result.returncode)
        print('PASS source codec roundtrip')
        result = run(COMPILER, 'build', enc, exe, '-k', key, '-c', '0xABC', timeout=30)
        assert result.returncode == 0, (result.stdout, result.returncode)
        assert run(exe).stdout == b'123\n'
        print('PASS native build from encoded source')
        for name, arguments in [
            ('key', ['-k', wrong_key, '-c', '0xABC']),
            ('dialect', ['-k', key, '-c', '0xABD']),
            ('missing_key', ['-c', '0xABC']),
            ('missing_dialect', ['-k', key]),
        ]:
            target = folder/(name+'.exe')
            result = run(COMPILER, 'build', enc, target, *arguments, timeout=30)
            assert result.returncode == 2 and not target.exists(), (name, result.stdout, result.returncode)
            print('PASS protected build rejects', name)
        damaged = bytearray(enc.read_bytes())
        damaged[12] ^= 1
        enc.write_bytes(damaged)
        result = run(COMPILER, 'build', enc, folder/'damaged.exe', '-k', key, '-c', '0xABC', timeout=30)
        assert result.returncode == 2 and not (folder/'damaged.exe').exists(), result.stdout
        print('PASS protected build rejects tampering')
        locked = folder/'locked.exe'
        locked.write_bytes(b'preserve existing contents')
        locked.chmod(stat.S_IREAD)
        try:
            result = run(COMPILER, 'build', src, locked)
            assert result.returncode == 2 and locked.read_bytes() == b'preserve existing contents', result.stdout
        finally:
            locked.chmod(stat.S_IWRITE | stat.S_IREAD)
        assert not list(folder.glob('aeg*.tmp')), 'temporary save file leaked'
        print('PASS failed output preserves existing file and removes temp')
        for args in [('building',src), ('build',src,exe,'-c','4096'), ('build',src,exe,'-c','nope'), ('render',enc,dec,'-k',key)]:
            result = run(COMPILER, *args)
            assert result.returncode == 2, (args, result.stdout, result.returncode)
        print('PASS invalid CLI commands and dialects')
    print(f'{len(cases) + len(errors) + 10} checks passed')


if __name__ == '__main__':
    main()
