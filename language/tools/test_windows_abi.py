"""Call generated scalar functions through the real Windows/libffi ABI.

Only embedded, pure arithmetic fixtures are loaded. The parent compiles each
fixture, checks its normal native result, disables its PE entry point and marks
it as a DLL. A separate Python child loads that DLL and calls the arithmetic
function with ctypes.WINFUNCTYPE. No foreign processes or files are inspected.
Aggregate/string layouts, callbacks, varargs and exception unwinding are outside
this scalar interoperability test and are not a public FFI implementation.
"""
import argparse
import ctypes
from ctypes import wintypes
import json
import math
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
COMPILER = Path(os.environ.get('AEGIS_COMPILER', ROOT/'target/debug/aegis-cli.exe'))
TYPES = {'i64': ctypes.c_int64, 'u64': ctypes.c_uint64,
         'f64': ctypes.c_double, 'f32': ctypes.c_float}
INSTRUCTION = re.compile(r'^[0-9A-F]{16}  file [0-9A-F]{8}  [0-9A-F ]{45} (.*)$', re.M)


def run(*arguments):
    result = subprocess.run(list(map(str, arguments)), capture_output=True, timeout=20,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    if result.returncode:
        raise AssertionError((arguments, result.returncode, result.stdout, result.stderr))
    return result.stdout


def instructions(compiler, executable, rva):
    listing = run(compiler, 'disasm', executable, hex(rva), '1024').decode('utf-8')
    rows = INSTRUCTION.findall(listing)
    if not rows:
        raise AssertionError('expected instruction rows in CLI disassembly')
    return rows


def first_call(compiler, executable, rva, image_base):
    for instruction in instructions(compiler, executable, rva):
        match = re.fullmatch(r'call ([0-9A-Fa-f]+)h', instruction)
        if match:
            return int(match[1], 16) - image_base
    raise AssertionError('fixture has no direct call to its arithmetic function')


def prepare_dll(compiler, executable, dll):
    data = bytearray(executable.read_bytes())
    nt = struct.unpack_from('<I', data, 0x3c)[0]
    optional = nt + 24
    if data[:2] != b'MZ' or data[nt:nt+4] != b'PE\0\0':
        raise AssertionError('fixture must be a PE image')
    if struct.unpack_from('<H', data, nt+4)[0] != 0x8664:
        raise AssertionError('fixture must be x64')
    entry = struct.unpack_from('<I', data, optional+16)[0]
    image_base = struct.unpack_from('<Q', data, optional+24)[0]
    main = first_call(compiler, executable, entry, image_base)
    target = first_call(compiler, executable, main, image_base)
    # Validate the selected target is file-backed executable code.
    sections = optional + struct.unpack_from('<H', data, nt+20)[0]
    valid = False
    for index in range(struct.unpack_from('<H', data, nt+6)[0]):
        section = sections + index * 40
        start, size = struct.unpack_from('<I', data, section+12)[0], struct.unpack_from('<I', data, section+16)[0]
        flags = struct.unpack_from('<I', data, section+36)[0]
        if start <= target < start + size and flags & 0x20000000:
            valid = True
    body = instructions(compiler, executable, target)
    if not valid or body[0] != 'push rbp':
        raise AssertionError('expected a generated arithmetic function')
    returned = False
    for instruction in body:
        if instruction.startswith('call ') or 'r15' in instruction or '[invalid]' in instruction:
            raise AssertionError('host-call fixture must not call the Aegis runtime')
        if instruction == 'ret':
            returned = True
            break
    if not returned:
        raise AssertionError('arithmetic function must return within decoding budget')
    struct.pack_into('<I', data, optional+16, 0)  # No DllMain / Aegis startup.
    flags = struct.unpack_from('<H', data, nt+22)[0]
    struct.pack_into('<H', data, nt+22, flags | 0x2000)  # IMAGE_FILE_DLL
    dll.write_bytes(data)
    return target


def invoke(manifest):
    spec = json.loads(manifest.read_text(encoding='utf-8'))
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.LoadLibraryW.argtypes = [wintypes.LPCWSTR]
    kernel.LoadLibraryW.restype = wintypes.HMODULE
    kernel.FreeLibrary.argtypes = [wintypes.HMODULE]
    kernel.FreeLibrary.restype = wintypes.BOOL
    module = kernel.LoadLibraryW(str(manifest.parent/spec['dll']))
    if not module:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        function = ctypes.WINFUNCTYPE(TYPES[spec['result']],
                                      *(TYPES[t] for t in spec['host_types']))(module + spec['rva'])
        actual = function(*spec['values'])
        expected = spec['expected']
        equal = actual == expected or (isinstance(expected, float)
                                        and math.isnan(expected) and math.isnan(actual))
        if not equal:
            raise AssertionError((spec['name'], actual, expected))
        if isinstance(expected, float) and expected == 0.0:
            if math.copysign(1.0, actual) != math.copysign(1.0, expected):
                raise AssertionError('zero sign was lost')
        print(json.dumps({'name': spec['name'], 'actual': actual, 'base': hex(module)}))
    finally:
        if not kernel.FreeLibrary(module):
            raise ctypes.WinError(ctypes.get_last_error())


def weighted_case(name, kinds):
    values = [(-11, 22, -33, 44)[i % 4] if kind == 'i64'
              else (1.25, -2.5, 3.75, -4.125)[i % 4] for i, kind in enumerate(kinds)]
    parameters = ','.join(f'p{i}:{kind}' for i, kind in enumerate(kinds))
    expression = '+'.join(f'f64(p{i})*{2**i}.0' for i in range(len(kinds)))
    arguments = ','.join(repr(v) for v in values)
    expected = sum(float(v) * 2**i for i, v in enumerate(values))
    source = f'fn probe({parameters})->f64 {{ return {expression}; }} fn main() {{ printi(probe({arguments})); }}'
    return name, source, 'f64', kinds, values, expected, f'{expected:.6f}\n'.encode()


def cases():
    result = [weighted_case(f'positions_{mask:04b}',
                            ['f64' if mask & (1 << i) else 'i64' for i in range(4)])
              for mask in range(16)]
    result += [weighted_case('single_precision_mix', ['i64', 'f32', 'f64', 'i64']),
               weighted_case('float_positions', ['f32', 'i64', 'f32', 'f64'])]
    for count in [5, 8, 32]:
        result.append(weighted_case(f'stack_{count}',
                                    [('i64', 'f64', 'f32', 'i64')[i % 4] for i in range(count)]))
    for kind, width, low, expected in [
        ('i8', 8, 0x80, -128), ('u8', 8, 0xf1, 241),
        ('i16', 16, 0x8001, -32767), ('u16', 16, 0xf123, 61731),
        ('i32', 32, 0x80000001, -2147483647), ('u32', 32, 0xf1234567, 4045620583),
        ('bool', 8, 0, 0), ('bool', 8, 1, 1),
    ]:
        polluted = (0xa123456789abcdef & ~((1 << width) - 1)) | low
        literal = ('true' if expected else 'false') if kind == 'bool' else str(expected)
        source = f'fn probe(a:{kind})->i64 {{ return i64(a); }} fn main() {{ printi(probe({literal})); }}'
        result.append((f'upper_bits_{kind}_{low}', source, 'i64', ['u64'], [polluted], expected,
                       f'{expected}\n'.encode()))
    result.append(('stack_narrow', 'fn probe(a:i64,b:i64,c:i64,d:i64,e:i8)->i64 { return i64(e); } fn main() { printi(probe(1,2,3,4,-128)); }',
                   'i64', ['i64'] * 4 + ['u64'], [1, 2, 3, 4, 0xa123456789abcd80], -128, b'-128\n'))
    result.append(('return_f32', 'fn probe(a:i64,b:f32,c:i64,d:f64)->f32 { return b; } fn main() { printi(probe(1,-0.0,3,4.0)); }',
                   'f32', ['i64', 'f32', 'i64', 'f64'], [1, -0.0, 3, 4.0], -0.0, b'-0.000000\n'))
    result.append(('default_f32', 'fn probe()->f32 {} fn main() { printi(probe()); }',
                   'f32', [], [], 0.0, b'0.000000\n'))
    for kind, value in [('i64', -9223372036854775808), ('u64', 18446744073709551615)]:
        source = f'fn probe(a:{kind})->{kind} {{ return a; }} fn main() {{ printi(probe({value})); }}'
        result.append((f'wide_{kind}', source, kind, [kind], [value], value, f'{value}\n'.encode()))
    for name, kind, expression, value, stdout in [
        ('nan_f64', 'f64', '0.0/0.0', math.nan, b'nan\n'),
        ('positive_infinity', 'f64', '1.0/0.0', math.inf, b'inf\n'),
        ('negative_infinity', 'f64', '-1.0/0.0', -math.inf, b'-inf\n'),
        ('nan_f32', 'f32', 'f32(0.0/0.0)', math.nan, b'nan\n'),
    ]:
        source = f'fn probe(a:{kind})->{kind} {{ return a; }} fn main() {{ printi(probe({expression})); }}'
        result.append((name, source, kind, [kind], [value], value, stdout))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--invoke', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--compiler', type=Path, default=COMPILER)
    args = parser.parse_args()
    if os.name != 'nt' or ctypes.sizeof(ctypes.c_void_p) != 8:
        parser.error('requires Windows x64 and a 64-bit Python interpreter')
    if args.invoke:
        invoke(args.invoke.resolve(strict=True))
        return
    compiler = args.compiler.resolve(strict=True)
    artifacts = ROOT/'artifacts'
    artifacts.mkdir(exist_ok=True)
    all_cases = cases()
    with tempfile.TemporaryDirectory(prefix='abi-', dir=artifacts) as temporary:
        folder = Path(temporary).resolve()
        if not folder.is_relative_to(artifacts.resolve()):
            raise AssertionError('fixture directory escaped the artifact directory')
        for name, source, result, host_types, values, expected, stdout in all_cases:
            src, exe, dll = folder/(name+'.ae'), folder/(name+'.exe'), folder/(name+'.dll')
            src.write_text(source, encoding='utf-8')
            run(compiler, 'build', src, exe)
            if run(exe) != stdout:
                raise AssertionError((name, 'native caller result'))
            rva = prepare_dll(compiler, exe, dll)
            manifest = folder/(name+'.json')
            manifest.write_text(json.dumps({'name': name, 'dll': dll.name, 'rva': rva,
                                            'result': result, 'host_types': host_types,
                                            'values': values, 'expected': expected}), encoding='utf-8')
            output = json.loads(run(sys.executable, Path(__file__).resolve(), '--invoke', manifest))
            if output['name'] != name:
                raise AssertionError('wrong child result')
            print('PASS Windows ABI', name, output['base'], flush=True)
    print(f'{len(all_cases)} Windows ABI interoperability checks passed')


if __name__ == '__main__':
    main()
