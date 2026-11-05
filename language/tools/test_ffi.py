"""Native unsafe-extern calls into own arithmetic DLLs and Windows APIs."""
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile

from test_windows_abi import cases, prepare_dll
from windows_aslr import run_relocated

ROOT = Path(__file__).resolve().parents[1]
COMPILER = Path(os.environ.get('AEGIS_COMPILER', ROOT/'target/debug/aegis-cli.exe')).resolve()


def run(*arguments, cwd=None):
    return subprocess.run(list(map(str, arguments)), cwd=cwd, capture_output=True,
                          timeout=20, creationflags=subprocess.CREATE_NO_WINDOW)


def compile_source(folder, name, source):
    src, exe = folder/(name+'.ae'), folder/(name+'.exe')
    src.write_text(source, encoding='utf-8')
    result = run(COMPILER, 'build', src, exe)
    if result.returncode:
        raise AssertionError((name, result.stdout, result.stderr, result.returncode))
    return exe


def export_probe(dll, target):
    data = bytearray(dll.read_bytes())
    nt = struct.unpack_from('<I', data, 0x3c)[0]
    optional = nt + 24
    sections = optional + struct.unpack_from('<H', data, nt+20)[0]
    for index in range(struct.unpack_from('<H', data, nt+6)[0]):
        section = sections + index * 40
        if data[section:section+8].rstrip(b'\0') == b'.rdata':
            size, rva, raw_size, raw = struct.unpack_from('<IIII', data, section+8)
            if size > 0x100 or raw_size < 0x200:
                raise AssertionError('fixture export table must not overwrite constants')
            struct.pack_into('<I', data, section+8, raw_size)
            break
    else:
        raise AssertionError('fixture has no .rdata section')
    struct.pack_into('<II', data, optional+0x70, rva+0x100, 0xa0)
    struct.pack_into('<IIIIIII', data, raw+0x10c,
                     rva+0x160, 1, 1, 1, rva+0x140, rva+0x148, rva+0x150)
    struct.pack_into('<I', data, raw+0x140, target)
    struct.pack_into('<I', data, raw+0x148, rva+0x180)
    struct.pack_into('<H', data, raw+0x150, 0)
    data[raw+0x160:raw+0x170] = b'ffi-fixture.dll\0'
    data[raw+0x180:raw+0x186] = b'Probe\0'
    dll.write_bytes(data)


def own_dll(folder, name, source):
    exe = compile_source(folder, name, source)
    dll = folder/(name+'.dll')
    target = prepare_dll(COMPILER, exe, dll)
    export_probe(dll, target)
    return dll


def check(folder, name, source, stdout, code=0):
    exe = compile_source(folder, name, source)
    result = run(exe, cwd=folder)
    if (result.returncode, result.stdout, result.stderr) != (code, stdout, b''):
        raise AssertionError((name, result.returncode, result.stdout, result.stderr))
    print('PASS FFI', name, flush=True)


def last_error_dll(folder, name='last_error', clobber_on_attach=False):
    # Independent C ABI producer: return numeric values while reading/writing
    # Win32's per-thread last-error state. No Aegis runtime or CRT is involved.
    program_files = Path(os.environ['ProgramFiles(x86)'])
    vswhere = program_files/'Microsoft Visual Studio/Installer/vswhere.exe'
    result = run(vswhere, '-latest', '-products', '*', '-requires',
                 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath', '-utf8')
    if result.returncode or not result.stdout.strip():
        raise AssertionError('MSVC x64 installation required for independent last-error fixture')
    installation = Path(result.stdout.decode('utf-8').strip())
    compilers = sorted((installation/'VC/Tools/MSVC').glob('*/bin/Hostx64/x64/cl.exe'))
    libraries = sorted((program_files/'Windows Kits/10/Lib').glob('*/um/x64/kernel32.lib'))
    if not compilers or not libraries:
        raise AssertionError('MSVC x64 compiler and Windows SDK required')
    src, obj, dll = folder/(name+'.c'), folder/(name+'.obj'), folder/(name+'.dll')
    source = '''__declspec(dllimport) void __stdcall SetLastError(unsigned long);
__declspec(dllimport) unsigned long __stdcall GetLastError(void);
__declspec(dllexport) unsigned long SetProbe(unsigned long value) { SetLastError(value); return value; }
__declspec(dllexport) unsigned long GetProbe(void) { return GetLastError(); }
'''
    if clobber_on_attach:
        source += 'int __stdcall AegisFixtureEntry(void* module, unsigned long reason, void* reserved) { if (reason == 1) SetLastError(287454020UL); return 1; }\n'
    src.write_text(source, encoding='ascii')
    result = run(compilers[-1], '/nologo', '/LD', '/GS-', '/O2', '/Fo'+str(obj), src,
                 '/link', '/ENTRY:AegisFixtureEntry' if clobber_on_attach else '/NOENTRY',
                 '/NODEFAULTLIB', '/INCREMENTAL:NO', '/OUT:'+str(dll),
                 '/IMPLIB:'+str(folder/(name+'.lib')), libraries[-1], cwd=folder)
    if result.returncode:
        raise AssertionError(('independent C fixture', result.stdout, result.stderr))
    return dll


def main():
    artifacts = ROOT/'artifacts'
    artifacts.mkdir(exist_ok=True)
    checks = 0
    with tempfile.TemporaryDirectory(prefix='ffi-', dir=artifacts) as temporary:
        folder = Path(temporary).resolve()
        if not folder.is_relative_to(artifacts.resolve()):
            raise AssertionError('fixture directory escaped artifacts')
        dll_folder = folder/'DLL space ünicode'
        dll_folder.mkdir()
        for name, source, _, _, _, _, stdout in cases():
            dll = own_dll(dll_folder, name, source)
            header = source[:source.index('{')].strip().replace('fn probe', 'fn foreign', 1)
            body = source.split('fn main()', 1)[1].strip()[1:-1].replace('probe(', 'foreign(')
            client = f'unsafe extern {json.dumps(str(dll), ensure_ascii=False)} {header} = "Probe"; fn main() {{ unsafe {{ {body} {body} }} }}'
            check(folder, name, client, stdout * 2)
            checks += 1
        raw = own_dll(dll_folder, 'raw_return', 'fn probe(a:u64)->u64 { return a; } fn main() { printi(probe(0)); }')
        path = json.dumps(str(raw), ensure_ascii=False)
        for kind, value, expected in [
            ('i8', 0xa123456789abcd80, -128), ('u8', 0xa123456789abcdf1, 241),
            ('i16', 0xa123456789ab8001, -32767), ('u16', 0xa123456789abf123, 61731),
            ('i32', 0xa123456780000001, -2147483647), ('u32', 0xa1234567f1234567, 4045620583),
            ('bool', 0xa123456789abcd00, 0), ('bool', 0xa123456789abcd01, 1),
        ]:
            client = f'unsafe extern {path} fn foreign(a:u64)->{kind} = "Probe"; fn main() {{ unsafe {{ printi(foreign({value})); printi(foreign({value})); }} }}'
            check(folder, f'return_{kind}_{expected}', client, f'{expected}\n{expected}\n'.encode())
            checks += 1
        check(folder, 'system32', 'unsafe extern "kernel32.dll" fn scale(a:i32,b:i32,c:i32)->i32="MulDiv"; fn main() { unsafe { printi(scale(-9,10,3)); printi(scale(9,10,3)); } }', b'-30\n30\n')
        checks += 1
        missing = 'unsafe extern "aegis-nonexistent-library.dll" fn missing()->i64="missing";'
        check(folder, 'unused_missing', missing+' fn main() { printi(7); }', b'7\n')
        check(folder, 'unreached_missing', missing+' fn main() { if(false) { unsafe { printi(missing()); } } printi(7); }', b'7\n')
        check(folder, 'used_missing', missing+' fn main() { printi(7); unsafe { printi(missing()); } }', b'7\n', 75)
        check(folder, 'missing_symbol', 'unsafe extern "kernel32.dll" fn missing()->i64="AegisDefinitelyMissingExport"; fn main() { unsafe { printi(missing()); } }', b'', 75)
        checks += 4
        fake = own_dll(folder, 'aegis-fake-system32-library', 'fn probe()->i64 { return 123; } fn main() { printi(probe()); }')
        fake_path = json.dumps(str(fake), ensure_ascii=False)
        check(folder, 'system32_loaded_basename', f'unsafe extern {fake_path} fn full()->i64="Probe"; unsafe extern "{fake.name}" fn bare()->i64="Probe"; fn main() {{ unsafe {{ printi(full()); printi(bare()); }} }}', b'123\n', 75)
        checks += 1
        declarations = ''.join(f'unsafe extern "kernel32.dll" fn external_fn_{i}(a:i32,b:i32,c:i32)->i32="MulDiv";' for i in range(64))
        calls = ''.join(f'sum=sum+external_fn_{i}(9,10,3);' for i in range(64))
        check(folder, 'all_64_slots', declarations+'fn main() { let sum:i32=0; unsafe { '+calls+' } printi(sum); }', b'1920\n')
        checks += 1
        state_dll = last_error_dll(folder)
        state_path = json.dumps(str(state_dll), ensure_ascii=False)
        getter_dll = last_error_dll(folder, 'last_error_getter', True)
        getter_path = json.dumps(str(getter_dll), ensure_ascii=False)
        setter = f'unsafe extern {state_path} fn set_error(value:u32)->u32="SetProbe";'
        body = 'fn main() { unsafe { set_error(305419896); let first=last_error(); printi(first); set_error(2271560481); printi(last_error()); } }'
        check(folder, 'last_error_system32', setter+'unsafe extern "kernel32.dll" fn last_error()->u32="GetLastError";'+body, b'305419896\n2271560481\n')
        check(folder, 'last_error_foreign', setter+f'unsafe extern {getter_path} fn last_error()->u32="GetProbe";'+body, b'305419896\n2271560481\n')
        checks += 2
        relocated = [
            ('system32', 'unsafe extern "kernel32.dll" fn scale(a:i32,b:i32,c:i32)->i32="MulDiv"; fn main() { unsafe { if(scale(-9,10,3)!=-30) { return 99; } } return 7; }'),
            ('absolute_unicode', f'unsafe extern {path} fn foreign(a:u64)->u64="Probe"; fn main() {{ unsafe {{ if(foreign(0xfedcba9876543210)!=0xfedcba9876543210) {{ return 99; }} }} return 7; }}'),
        ]
        for name, source in relocated:
            exe = compile_source(folder, 'relocated_'+name, source)
            data = exe.read_bytes()
            nt = struct.unpack_from('<I', data, 0x3c)[0]
            preferred = struct.unpack_from('<Q', data, nt+24+24)[0]
            base, code = run_relocated(exe)
            if base == preferred or base <= 0xffffffff or code != 7:
                raise AssertionError(('FFI mandatory ASLR', name, hex(base), code))
            print('PASS FFI mandatory ASLR', name, hex(base), flush=True)
            checks += 1
    print(f'{checks} native FFI checks passed')


if __name__ == '__main__':
    main()
