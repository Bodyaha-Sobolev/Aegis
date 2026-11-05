"""Run compiler-generated enum programs, including guarded ownership paths."""
from pathlib import Path
import os
import subprocess
import tempfile
from windows_aslr import run_relocated

ROOT = Path(__file__).resolve().parents[1]
COMPILER = Path(os.environ.get('AEGIS_COMPILER', ROOT / 'target/debug/aegis-cli.exe'))


def run(exe, *args):
    return subprocess.run([str(exe), *map(str, args)], capture_output=True,
                          timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)


def cases():
    header = 'enum E { Text(str), Number(i64), Empty } '
    show = 'fn show(e:E) { match e { E::Text(t)=>{println(t);}, E::Number(n)=>{printi(n);}, E::Empty=>{printi(0);} } } '
    yield 'variants', header+show+'fn main(){show(E::Text("a"+"b")); show(E::Number(5)); show(E::Empty);}', b'ab\n5\n0\n'
    yield 'copy_switch_self', header+show+'fn main(){let a=E::Text("a"+"b"); let b=a; a=E::Number(5); show(b); show(a); a=b; a=a; show(a); b=E::Empty; show(a);}', b'ab\n5\nab\nab\n'
    yield 'array_index_repeat', header+show+'fn main(){let a=[E::Text("a"+"b");3]; let b=a; a[1]=E::Number(3); a[0]=a[2]; show(a[0]); show(a[1]); show(b[1]);}', b'ab\n3\nab\n'
    yield 'return_binding', header+'fn take(e:E)->str {match e {E::Text(t)=>{return t;}, E::Number(_)=>{return "number";}, E::Empty=>{return "empty";}}} fn main(){println(take(E::Text("heap"+"!")));}', b'heap!\n'
    yield 'nested_guards', header+'enum Outer { Inner(E), Number(i64), Empty } fn main(){let a=Outer::Inner(E::Text("a"+"b")); let b=a; a=Outer::Number(1); a=Outer::Empty; match b { Outer::Inner(e)=>{match e {E::Text(t)=>{println(t);}, E::Number(_)=>{}, E::Empty=>{}}}, Outer::Number(_)=>{}, Outer::Empty=>{} }}', b'ab\n'
    yield 'record_dyn_payload', 'struct P {name:str, items:[str]} enum R { Data(P), Error(u32) } fn main(){let r=R::Data(P{name:"a"+"b",items:["x"+"y"]}); match r {R::Data(p)=>{p.name=p.name+"!"; p.items[0]="new"+"!"; println(p.name); println(p.items[0]);}, R::Error(_)=>{}} match r {R::Data(p)=>{println(p.name); println(p.items[0]);}, R::Error(_)=>{}}}', b'ab!\nnew!\nab\nxy\n'
    yield 'loop_cleanup', header+'fn main(){let i=0; while(i<5){i=i+1; match E::Text("heap"+"!"){E::Text(t)=>{if(i<3){continue;} println(t); break;}, E::Number(_)=>{}, E::Empty=>{}}} printi(i);}', b'heap!\n3\n'
    yield 'discard_stress', header+'struct P {s:str} fn make()->E{return E::Text("hello"+"!");} fn record()->P{return P{s:"hello"+"!"};} fn main(){let i=0; while(i<100000){make(); record(); E::Text("a"+"b"); i=i+1;} printi(i);}', b'100000\n'
    yield 'numeric_payloads', 'enum F { Small(i8), Float(f64), Flag(bool) } fn main(){match F::Small(-128){F::Small(n)=>{printi(n);}, F::Float(_)=>{}, F::Flag(_)=>{}} match F::Float(1.5){F::Small(_)=>{}, F::Float(n)=>{printi(n==1.5);}, F::Flag(_)=>{}} match F::Flag(true){F::Small(_)=>{}, F::Float(_)=>{}, F::Flag(n)=>{printi(n);}}}', b'-128\n1\n1\n'
    yield 'import_return', 'mod m {enum E { Ok(str), Bad }} use m::E as Result; fn f()->Result{return Result::Ok("o"+"k");} fn main(){match f(){Result::Ok(t)=>{println(t);}, Result::Bad=>{}}}', b'ok\n'
    variants = ','.join(f'V{i}' for i in range(128))
    arms = ','.join(f'E::V{i}=>{{printi({i});}}' for i in range(128))
    yield 'all_128_variants', f'enum E {{{variants}}} fn main(){{match E::V127 {{{arms}}}}}', b'127\n'


def memory_cases():
    # Each iteration allocates >4 KiB. 30,000 unreclaimed allocations would
    # exceed 117 MiB, far above the child's 32 MiB commit limit.
    text = '"' + 'x'*4096 + '"+"!"'
    header = 'enum E { Text(str), Number(i64), Empty } '
    yield 'discard_enum', header+f'fn make()->E{{return E::Text({text});}} fn main(){{let i=0; while(i<30000){{make(); i=i+1;}} return 7;}}'
    yield 'discard_record', f'struct P{{s:str}} fn make()->P{{return P{{s:{text}}};}} fn main(){{let i=0; while(i<30000){{make(); i=i+1;}} return 7;}}'
    yield 'nested_copy_switch', header+'enum Outer { Value(E), Number(i64) } '+f'fn main(){{let i=0; while(i<30000){{let a=[Outer::Value(E::Text({text}));3]; let b=a; a[1]=Outer::Number(1); a[0]=a[2]; i=i+1;}} return 7;}}'
    yield 'match_return', header+'fn take(e:E)->str{match e{E::Text(t)=>{return t;}, E::Number(_)=>{return "";}, E::Empty=>{return "";}}} '+f'fn main(){{let i=0; while(i<30000){{let s=take(E::Text({text})); if(len(s)!=4097){{return 1;}} i=i+1;}} return 7;}}'
    yield 'loop_control', header+f'fn main(){{let i=0; while(i<30000){{i=i+1; match E::Text({text}){{E::Text(t)=>{{if(len(t)!=4097){{return 1;}} if(i<30000){{continue;}} break;}}, E::Number(_)=>{{}}, E::Empty=>{{}}}}}} return 7;}}'


def main():
    root = (ROOT / 'artifacts').resolve()
    root.mkdir(exist_ok=True)
    count = 0
    with tempfile.TemporaryDirectory(prefix='enum-tests-', dir=root) as temporary:
        folder = Path(temporary).resolve()
        assert folder.is_relative_to(root)
        for name, source, expected in cases():
            src, exe = folder / (name+'.ae'), folder / (name+'.exe')
            src.write_text(source, encoding='utf-8')
            built = run(COMPILER, 'build', src, exe)
            assert built.returncode == 0, (name, built.stdout, built.stderr)
            result = run(exe)
            assert (result.returncode, result.stdout) == (0, expected), (name, result.returncode, result.stdout, result.stderr)
            print('PASS enum', name)
            count += 1
        src, exe = folder / 'relocated.ae', folder / 'relocated.exe'
        src.write_text('enum E { Text(str), Number(i64) } fn main(){let a=E::Text("a"+"b"); let b=a; a=E::Number(3); match b {E::Text(t)=>{if(t[1]==98){return 7;}}, E::Number(_)=>{}} return 1;}', encoding='utf-8')
        built = run(COMPILER, 'build', src, exe)
        assert built.returncode == 0, (built.stdout, built.stderr)
        base, code = run_relocated(exe)
        assert base > 0xffffffff and code == 7, (base, code)
        count += 1
        print('PASS enum forced ASLR')
        cap = 32*1024*1024
        for name, source in memory_cases():
            src, exe = folder / (name+'.ae'), folder / (name+'.exe')
            src.write_text(source, encoding='utf-8')
            built = run(COMPILER, 'build', src, exe)
            assert built.returncode == 0, (name, built.stdout, built.stderr)
            _, code = run_relocated(exe, timeout=30, memory_limit=cap)
            assert code == 7, (name, 'bounded ownership workload failed', code)
            count += 1
            print('PASS enum 32 MiB commit cap', name)
        # Positive control: these allocations remain reachable. The same valid
        # program must succeed normally and fail allocation under the cap.
        source = 'fn main(){let a:[str]=[]; let i=0; while(i<20000){a=push(a,"'+'x'*4096+'"+"!"); i=i+1;} if(len(a)!=20000){return 1;} return 7;}'
        src, exe = folder / 'retained.ae', folder / 'retained.exe'
        src.write_text(source, encoding='utf-8')
        built = run(COMPILER, 'build', src, exe)
        assert built.returncode == 0, (built.stdout, built.stderr)
        assert run(exe).returncode == 7, 'retained control must succeed uncapped'
        _, code = run_relocated(exe, timeout=30, memory_limit=cap)
        assert code == 74, ('commit cap did not reject retained allocations', code)
        count += 2
        print('PASS memory cap retained-allocation control (uncapped and capped)')
    print(f'{count} enum native checks passed')


if __name__ == '__main__':
    main()
