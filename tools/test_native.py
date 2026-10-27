"""End-to-end tests: compile AEGIS source, run native PE output, check diagnostics."""
from pathlib import Path
import os
import stat
import subprocess
import tempfile
from windows_aslr import run_relocated
from pe_fixture import export_fixture

ROOT = Path(__file__).resolve().parents[1]
COMPILER = Path(os.environ.get('AEGIS_COMPILER', ROOT / 'target' / 'debug' / 'aegis-cli.exe'))


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
        ('short_circuit', 'fn main() { printi(0 && 1/0); printi(1 || 1/0); printi(1 && 2); printi(0 || 3); return 0; }', b'0\n1\n1\n1\n', 0),
        ('loop_control', 'fn main() { let i=0; while(i<10) { i=i+1; if(i==2) { continue; } if(i==4) { break; } printi(i); } return 0; }', b'1\n3\n', 0),
        ('division_zero', 'fn main() { return 1/0; }', b'', 70),
        ('division_overflow', 'fn main() { return -9223372036854775808/-1; }', b'', 71),
    ]
    cases.extend([
        ('repeat_once', 'fn seed() { printi(7); return 42; } fn main() { let a=[seed();4]; printi(a[0]); printi(a[3]); }', b'7\n42\n42\n', 0),
        ('repeat_assignment_snapshot', 'fn main() { let a=[1,2,3]; a=[a[0]+a[2];3]; printi(a[0]); printi(a[1]); printi(a[2]); }', b'4\n4\n4\n', 0),
        ('repeat_argument_once', 'fn seed() { printi(1); return 9; } fn f(a:[i64;3],b) { printi(a[0]); printi(a[2]); printi(b); } fn main() { f([seed();3],seed()); }', b'1\n1\n9\n9\n9\n', 0),
        ('repeat_full_frame', 'fn main() { let a=[13;256]; printi(a[0]); printi(a[255]); printi(len(a)); }', b'13\n13\n256\n', 0),
        ('repeat_full_argument', 'fn f(a:[i64;256]) { printi(a[0]); printi(a[255]); a[0]=99; printi(a[0]); } fn main() { f([17;256]); }', b'17\n17\n99\n', 0),
        ('repeat_types', 'fn f(a:[str;2],b:[u64;2],c:[bool;2]) { println(a[1]); printi(b[1]); printi(c[1]); } fn main() { f(["ok";2],[0xffffffffffffffff;2],[true;2]); }', b'ok\n18446744073709551615\n1\n', 0),
        ('repeat_fault', 'fn main() { let a=[1/0;8]; printi(a[0]); }', b'', 70),
        ('repeat_bounds', 'fn main() { let a=[42;2]; printi(a[2]); }', b'', 72),
    ])
    cases.extend([
        ('struct_copy', 'struct P { x:i64,y:i64 } fn main() { let a=P { x:2,y:3 }; let b=a; b.x=9; printi(a.x); printi(b.x); a=b; printi(a.x); a=P { y:a.x,x:a.y }; printi(a.x); printi(a.y); }', b'2\n9\n9\n3\n9\n', 0),
        ('struct_argument', 'struct P { x:i64,y:i64 } fn f(p:P) { printi(p.x); p.x=99; printi(p.x); } fn main() { let a=P { x:7,y:8 }; f(a); printi(a.x); f(P { y:2,x:1 }); }', b'7\n99\n7\n1\n99\n', 0),
        ('struct_order', 'struct P { x:i64,y:i64 } fn seed(n) { printi(n); return n; } fn f(p:P,n) { printi(p.x); printi(p.y); printi(n); } fn main() { f(P { y:seed(2),x:seed(1) },seed(3)); }', b'2\n1\n3\n1\n2\n3\n', 0),
        ('struct_types', 'struct P { name:str, mask:u64, active:bool } fn f(p:P)->u64 { println(p.name); printi(p.active); return p.mask; } fn main() { let p=P { mask:0xffffffffffffffff, active:true, name:"hero" }; printi(f(p)); p.name="new"; println(p.name); printi(p.name[1]); }', b'hero\n1\n18446744073709551615\nnew\n101\n', 0),
        ('struct_stack_args', 'struct P { x:i64,y:i64 } fn f(a,b,c,d,p:P,q:P,t:[i64;2]) { printi(a+b+c+d+p.x+p.y+q.x+q.y+t[0]+t[1]); p.x=99; } fn main() { let p=P { x:5,y:6 }; f(1,2,3,4,p,P { y:8,x:7 },[9,10]); printi(p.x); }', b'55\n5\n', 0),
        ('struct_recursive', 'struct P { x:i64,y:i64 } fn sum(p:P,n) { if(n==0) { return p.x+p.y; } p.x=p.x+1; return sum(p,n-1); } fn main() { let p=P { x:1,y:2 }; printi(sum(p,8)); printi(p.x); }', b'11\n1\n', 0),
        ('struct_single', 'struct P { x:i64 } fn f(p:P,a,b,c,d) { printi(p.x+a+b+c+d); p.x=99; } fn main() { let p=P { x:1 }; f(p,2,3,4,5); printi(p.x); }', b'15\n1\n', 0),
    ])
    record_fields = ','.join(f'f{i}:i64' for i in range(256))
    record_values = ','.join(f'f{i}:{i}' for i in reversed(range(256)))
    cases.append(('struct_full_layout', 'struct P {'+record_fields+'} fn main() { let p=P {'+record_values+'}; printi(p.f0); printi(p.f255); p.f255=42; printi(p.f255); }', b'0\n255\n42\n', 0))
    cases.append(('struct_full_argument', 'struct P {'+record_fields+'} fn f(p:P) { printi(p.f0); printi(p.f255); p.f255=42; printi(p.f255); } fn main() { f(P {'+record_values+'}); }', b'0\n255\n42\n', 0))
    cases.extend([
        ('nested_struct_copy', 'struct E { p:P, n:i64 } struct P { x:i64,y:i64 } fn main() { let a=E { n:7,p:P { x:2,y:3 } }; let b=a; b.p.x=9; printi(a.p.x); printi(b.p.x); a.p=b.p; printi(a.p.x); a.p=P { y:a.p.x,x:a.p.y }; printi(a.p.x); printi(a.p.y); printi(a.n); }', b'2\n9\n9\n3\n9\n7\n', 0),
        ('nested_struct_argument', 'struct E { p:P, a:[i64;3] } struct P { x:i64,y:i64 } fn f(e:E) { printi(e.p.x+e.p.y+e.a[2]); e.p.x=99; e.a[0]=88; } fn main() { let e=E { a:[4;3],p:P { y:2,x:1 } }; f(e); printi(e.p.x); printi(e.a[0]); f(E { p:e.p,a:e.a }); }', b'7\n1\n4\n7\n', 0),
        ('array_field_swap', 'struct E { a:[i64;3], b:[i64;3] } fn main() { let e=E { a:[1,2,3], b:[4;3] }; e.a=[e.a[2],e.a[1],e.a[0]]; printi(e.a[0]); printi(e.a[2]); e.b=e.a; e.a[0]=99; printi(e.b[0]); printi(len(e.a)); }', b'3\n1\n3\n3\n', 0),
        ('array_field_arguments', 'struct E { a:[i64;3] } fn f(a:[i64;3]) { printi(a[0]+a[2]); a[0]=99; } fn main() { let e=E { a:[5;3] }; f(e.a); printi(e.a[0]); }', b'10\n5\n', 0),
        ('nested_evaluation_order', 'struct P { a:i64,b:i64 } struct E { p:P,v:[i64;2],c:i64 } fn seed(n) { printi(n); return n; } fn f(e:E,n) { printi(e.p.a+e.p.b+e.v[0]+e.v[1]+e.c+n); } fn main() { f(E { v:[seed(1),seed(2)],p:P { b:seed(3),a:seed(4) },c:seed(5) },seed(6)); }', b'1\n2\n3\n4\n5\n6\n21\n', 0),
        ('nested_string_fields', 'struct P { s:str } struct E { p:P,names:[str;2],bits:[u64;2] } fn main() { let e=E { bits:[0xffffffffffffffff;2], names:["one","two"], p:P { s:"hi" } }; println(e.p.s); printi(e.p.s[1]); println(e.names[1]); printi(e.bits[0]); e.p.s="changed"; println(e.p.s); }', b'hi\n105\ntwo\n18446744073709551615\nchanged\n', 0),
        ('array_field_bounds', 'struct E { a:[i64;2] } fn main() { let e=E { a:[1;2] }; printi(e.a[-1]); }', b'', 72),
        ('array_field_check_order', 'struct E { a:[i64;2] } fn seed() { printi(99); return 1; } fn main() { let e=E { a:[1;2] }; e.a[2]=seed(); }', b'', 72),
        ('nested_full_frame', 'struct P { a:[i64;128] } struct E { left:P,right:P } fn f(e:E) { printi(e.left.a[127]); printi(e.right.a[127]); } fn main() { let e=E { right:P { a:[2;128] },left:P { a:[1;128] } }; f(e); e.left=e.right; printi(e.left.a[127]); }', b'1\n2\n2\n', 0),
    ])
    cases.extend([
        ('struct_array_values', 'struct P { x:i64,y:i64 } fn main() { let a=[P { x:1,y:2 },P { y:4,x:3 }]; printi(len(a)); printi(a[1].y); a[0]=a[1]; a[1].x=99; printi(a[0].x); printi(a[1].x); }', b'2\n4\n3\n99\n', 0),
        ('struct_array_repeat_once', 'struct P { x:i64,y:i64 } fn seed(n) { printi(n); return n; } fn main() { let a=[P { y:seed(2),x:seed(1) };3]; printi(a[2].x+a[2].y); a[1].x=99; printi(a[0].x); }', b'2\n1\n3\n1\n', 0),
        ('struct_array_arguments', 'struct P { x:i64,y:i64 } fn f(a:[P;2],b,c,d,e) { printi(a[0].x+a[1].y+b+c+d+e); a[0].x=99; } fn main() { let a=[P { x:1,y:2 };2]; f(a,3,4,5,6); printi(a[0].x); }', b'21\n1\n', 0),
        ('matrix_copy', 'fn main() { let a=[[1,2,3],[4,5,6]]; printi(len(a)); printi(len(a[0])); a[0]=a[1]; a[1][0]=99; printi(a[0][0]); printi(a[1][0]); a[0]=[a[0][2],a[0][1],a[0][0]]; printi(a[0][0]); printi(a[0][2]); }', b'2\n3\n4\n99\n6\n4\n', 0),
        ('matrix_repeat_once', 'fn seed() { printi(7); return 3; } fn main() { let a=[[seed();2];3]; printi(a[2][1]); a[1][0]=99; printi(a[0][0]); }', b'7\n3\n3\n', 0),
        ('matrix_argument', 'fn f(a:[[u64;2];2])->u64 { a[0][0]=0; return a[1][1]; } fn main() { let a:[[u64;2];2]=[[0xffffffffffffffff;2];2]; printi(f(a)); printi(a[0][0]); }', b'18446744073709551615\n18446744073709551615\n', 0),
        ('composite_world', 'struct P { x:i64, tags:[str;2] } struct W { actors:[P;2] } fn main() { let w=W { actors:[P { tags:["a","b"],x:7 };2] }; println(w.actors[1].tags[0]); w.actors[1].tags[0]="new"; println(w.actors[0].tags[0]); println(w.actors[1].tags[0]); printi(w.actors[1].tags[1][0]); }', b'a\na\nnew\n98\n', 0),
        ('matrix_inner_bounds', 'fn main() { let a=[[1;3];2]; printi(a[0][3]); }', b'', 72),
        ('matrix_outer_bounds', 'fn main() { let a=[[1;3];2]; printi(a[2][0]); }', b'', 72),
        ('matrix_assignment_bounds_order', 'fn seed() { printi(99); return 1; } fn main() { let a=[[1;3];2]; a[2]=[seed();3]; }', b'', 72),
        ('matrix_index_order', 'fn seed(n) { printi(n); return n; } fn main() { let a=[[1;3];2]; printi(a[seed(1)][seed(2)]); }', b'1\n2\n1\n', 0),
        ('len_preserves_index_effects', 'fn seed() { printi(7); return 0; } fn main() { let a=[[1;3];2]; printi(len(a[seed()])); }', b'7\n3\n', 0),
        ('len_preserves_bounds', 'fn main() { let a=[[1;3];2]; printi(len(a[2])); }', b'', 72),
        ('composite_full_frame', 'struct P { x:i64,y:i64 } fn f(a:[P;128]) { printi(a[127].x); printi(a[127].y); } fn main() { let a=[P { y:2,x:1 };128]; f(a); a[127]=a[0]; printi(a[127].y); }', b'1\n2\n2\n', 0),
    ])
    cases.extend([
        ('return_struct', 'struct P { x:i64,s:str } fn make(n)->P { return P { s:"hello",x:n }; } fn main() { let p=make(42); println(p.s); printi(p.x); }', b'hello\n42\n', 0),
        ('return_array', 'fn make(n)->[i64;3] { return [n,n+1,n+2]; } fn main() { let a=make(7); printi(a[0]); printi(a[2]); a=make(9); printi(a[1]); }', b'7\n9\n10\n', 0),
        ('return_nested_calls', 'struct P { x:i64,y:i64 } fn make(n)->P { return P { x:n,y:n+1 }; } fn update(p:P)->P { p.x=p.x+10; return p; } fn score(p:P) { return p.x+p.y; } fn main() { let p=update(make(2)); printi(score(update(p))); printi(p.x); printi(score(make(5))); }', b'25\n12\n11\n', 0),
        ('return_repeat_once', 'struct P { x:i64 } fn make()->P { printi(7); return P { x:9 }; } fn main() { let a=[make();3]; printi(a[2].x); }', b'7\n9\n', 0),
        ('return_recursive', 'struct P { x:i64 } fn make(n)->P { if(n==0) { return P { x:7 }; } let p=make(n-1); p.x=p.x+1; return p; } fn main() { let p=make(64); printi(p.x); }', b'71\n', 0),
        ('return_branches', 'fn make(n)->[str;2] { if(n) { return ["yes";2]; } else { return ["no";2]; } } fn main() { let a=make(1); let b=make(0); println(a[1]); println(b[0]); }', b'yes\nno\n', 0),
        ('return_stack_arguments', 'struct P { x:i64,y:i64 } fn make(a,b,c,d,e,f)->P { return P { y:f,x:a+b+c+d+e }; } fn main() { let p=make(1,2,3,4,5,6); printi(p.x); printi(p.y); }', b'15\n6\n', 0),
        ('return_matrix', 'fn make()->[[u64;2];2] { return [[0xffffffffffffffff;2];2]; } fn main() { let a=make(); printi(a[1][1]); }', b'18446744073709551615\n', 0),
        ('return_full_frame', 'fn make(a:[i64;256])->[i64;256] { a[255]=99; return a; } fn main() { let a=make([7;256]); printi(a[0]); printi(a[255]); }', b'7\n99\n', 0),
        ('return_discard_cleanup', 'struct P { x:i64 } fn make()->P { return P { x:7 }; } fn main() { let i=0; while(i<500000) { make(); i=i+1; } let p=make(); printi(p.x); }', b'7\n', 0),
        ('return_field_assignment', 'struct P { x:i64 } struct E { p:P } fn make(n)->P { return P { x:n }; } fn main() { let e=E { p:make(1) }; e.p=make(42); printi(e.p.x); }', b'42\n', 0),
    ])
    return_params=','.join(f'a{i}' for i in range(32))
    return_values=','.join(str(i) for i in range(32))
    return_sum='+'.join(f'a{i}' for i in range(32))
    cases.append(('return_32_arguments', 'struct P { x:i64 } fn make('+return_params+')->P { return P { x:'+return_sum+' }; } fn main() { let p=make('+return_values+'); printi(p.x); }', b'496\n', 0))
    cases.append(('return_nested_constructor', 'struct P { x:i64 } struct W { items:[P;2],name:str } fn make(n)->P { printi(n); return P { x:n }; } fn world()->W { return W { name:"world",items:[make(1),make(2)] }; } fn main() { let w=world(); println(w.name); printi(w.items[0].x+w.items[1].x); }', b'1\n2\nworld\n3\n', 0))
    cases.extend([
        ('module_functions', 'mod math { fn double(n) { return n*2; } } fn main() { printi(math::double(21)); }', b'42\n', 0),
        ('module_lexical_lookup', 'fn value() { return 1; } mod a { fn value() { return 2; } mod b { fn run() { printi(value()); printi(::value()); } } } fn main() { a::b::run(); }', b'2\n1\n', 0),
        ('module_local_shadow', 'fn value() { return 1; } mod a { fn value() { return 2; } mod b { fn value() { return 3; } fn run() { printi(value()); printi(::a::value()); printi(::value()); } } } fn main() { a::b::run(); }', b'3\n2\n1\n', 0),
        ('module_types_return', 'mod a { struct P { x:i64,s:str } fn make(n)->P { return P { s:"a",x:n }; } } mod b { struct P { x:i64 } fn make(n)->P { return P { x:n+1 }; } } fn main() { let a=a::make(7); let b=b::make(7); println(a.s); printi(a.x); printi(b.x); }', b'a\n7\n8\n', 0),
        ('module_cross_types', 'mod a { struct P { x:i64 } } mod b { struct W { items:[a::P;2] } fn make()->W { return W { items:[a::P { x:9 };2] }; } } fn main() { let w=b::make(); printi(w.items[1].x); }', b'9\n', 0),
        ('module_recursion', 'mod a { fn even(n)->bool { if(n==0) { return true; } return b::odd(n-1); } } mod b { fn odd(n)->bool { if(n==0) { return false; } return a::even(n-1); } } fn main() { printi(a::even(20)); printi(b::odd(20)); }', b'1\n0\n', 0),
        ('module_sibling_relative', 'mod a { mod math { fn add(x,y) { return x+y; } } mod game { fn score(n) { return math::add(n,2); } } } fn main() { printi(a::game::score(40)); }', b'42\n', 0),
        ('module_type_shadow', 'struct P { x:i64 } mod a { struct P { x:bool } fn make()->::P { let p=P { x:true }; printi(p.x); return ::P { x:42 }; } } fn main() { let p=a::make(); printi(p.x); }', b'1\n42\n', 0),
    ])
    errors = [
        ('empty_binary', 'fn main() { return 0b; }', b'integer literal'),
        ('bad_binary', 'fn main() { return 0b102; }', b'ERROR'),
        ('unfinished_shift', 'fn main() { return 1 >>>; }', b'expected expression'),
        ('undefined', 'fn main() { printi(missing); }', b'undefined variable'),
        ('no_main', 'fn other() {}', b'fn main() is required'),
        ('unclosed', 'fn main() { return 0;', b'ERROR 1:'),
        ('duplicate_fn', 'fn main() {} fn main() {}', b'function already defined'),
        ('duplicate_var', 'fn main() { let x=1; let x=2; }', b'variable already defined'),
        ('arity', 'fn f(x) { return x; } fn main() { return f(); }', b'wrong number'),
        ('trailing', 'fn main() {} 42', b'expected function definition'),
        ('comment', 'fn main() {} /*', b'unterminated block comment'),
        ('overflow', 'fn main() { return 18446744073709551616; }', b'integer literal'),
        ('scope', 'fn main() { if(1) { let x=42; } printi(x); }', b'undefined variable'),
        ('position', 'fn main() {\n  printi(missing);\n}', b'ERROR 2:10:'),
        ('bad_fn', 'fn main() { return missing(); }', b'undefined function'),
        ('depth', 'fn main() { return ' + '(' * 140 + '1' + ')' * 140 + '; }', b'limit exceeded'),
        ('long_chain', 'fn main() { return ' + '+'.join(['1']*1000) + '; }', b'depth limit exceeded'),
        ('break_outside', 'fn main() { break; }', b'break is only valid'),
    ]
    # Each vector runs through both emitted machine instructions (function
    # parameters) and constant folding. Python supplies an independent oracle.
    mask = (1 << 64) - 1
    def signed(n):
        n &= mask
        return n if n < (1 << 63) else n - (1 << 64)
    values = [0, 1, 2, 31, 63, 64, 65, 127, mask, 1 << 63, 0x123456789ABCDEF0]
    operations = {
        '&': lambda a,b: a & b, '|': lambda a,b: a | b,
        '^': lambda a,b: a ^ b, '<<': lambda a,b: a << (b & 63),
        '>>': lambda a,b: signed(a) >> (b & 63),
        '>>>': lambda a,b: a >> (b & 63),
    }
    for index, (op, oracle) in enumerate(operations.items()):
        body, expected = [], []
        for i, a in enumerate(values):
            for b in [values[(i + 3) % len(values)], 0, 63, 64, mask]:
                body.extend([f'printi(calc({a},{b}));', f'printi({a} {op} {b});'])
                expected.extend([str(signed(oracle(a,b)))] * 2)
        source = f'fn calc(a,b) {{ return a {op} b; }} fn main() {{' + ''.join(body) + 'return 0; }'
        cases.append((f'bit_oracle_{index}', source, ('\n'.join(expected)+'\n').encode(), 0))
    cases.extend([
        ('array_sorting', (ROOT/'examples'/'arrays.ae').read_text(encoding='utf-8'), b'1\n2\n3\n5\n8\n9\n8\n', 0),
        ('array_copy', 'fn main() { let a=[1,2,3]; let b=a; b[0]=9; printi(a[0]); printi(b[0]); a=b; b[1]=8; printi(a[1]); printi(b[1]); a=a; printi(a[0]); }', b'1\n9\n2\n8\n9\n', 0),
        ('array_snapshot', 'fn main() { let a=[1,2,3]; a=[a[2],a[0],a[1]]; printi(a[0]); printi(a[1]); printi(a[2]); }', b'3\n1\n2\n', 0),
        ('array_unsigned', 'fn main() { let a:[u64;2]=[0xffffffffffffffff,0x8000000000000000]; printi(a[0]); printi(a[1]/2); printi(a[0]>a[1]); let b=[true,false]; b[1]=!b[0]; printi(b[1]); }', b'18446744073709551615\n4611686018427387904\n1\n0\n', 0),
        ('array_call_order', 'fn mark(x) { printi(x); return x; } fn main() { let a=[mark(1),mark(2)]; a[mark(0)]=mark(3); printi(a[0]); printi(a[1]); }', b'1\n2\n0\n3\n3\n2\n', 0),
        ('array_negative_read', 'fn main() { let a=[1,2]; printi(a[-1]); }', b'', 72),
        ('array_past_end_read', 'fn main() { let a=[1,2]; printi(a[len(a)]); }', b'', 72),
        ('array_unsigned_index', 'fn main() { let a=[1,2]; printi(a[u64(-1)]); }', b'', 72),
        ('array_bounds_before_rhs', 'fn mark(x) { printi(x); return x; } fn main() { let a=[1]; a[1]=mark(99); }', b'', 72),
        ('array_negative_write', 'fn main() { let a=[1,2]; a[-1]=3; }', b'', 72),
        ('array_reused_scope', 'fn main() { let x=7; {let a=[1,2]; printi(a[1]);} {let b=[3,4]; printi(b[0]);} printi(x); }', b'2\n3\n7\n', 0),
        ('five_args', 'fn sum(a,b,c,d,e) { return a+b+c+d+e; } fn main() { printi(sum(1,2,3,4,5)); printi(10+sum(1,2,3,4,5)); }', b'15\n25\n', 0),
        ('stack_arg_order', 'fn mark(x) { printi(x); return x; } fn sum(a,b,c,d,e,f) { return a+10*b+100*c+1000*d+10000*e+100000*f; } fn main() { printi(sum(mark(1),mark(2),mark(3),mark(4),mark(5),mark(6))); }', b'1\n2\n3\n4\n5\n6\n654321\n', 0),
        ('nested_stack_args', 'fn sum(a,b,c,d,e) { return a+b+c+d+e; } fn main() { printi(1+sum(sum(1,2,3,4,5),2,sum(1,1,1,1,1),4,sum(2,2,2,2,2))*2); }', b'73\n', 0),
        ('recursive_stack_args', 'fn sum(n,a,b,c,d,e) { if(n==0) { return a+b+c+d+e; } return sum(n-1,a+1,b+1,c+1,d+1,e+1); } fn main() { printi(sum(20,1,2,3,4,5)); }', b'115\n', 0),
        ('compact_recursion', 'fn count(n) { if(n==0) { return 0; } return count(n-1)+1; } fn main() { printi(count(5000)); }', b'5000\n', 0),
        ('reused_scopes', 'fn main() { let x=7;' + '{ let y=x+1; printi(y); }'*300 + 'printi(x); }', b'8\n'*300+b'7\n', 0),
        ('bit_not', 'fn invert(x) { return ~x; } fn main() { printi(~0); printi(invert(0)); printi(~0x8000000000000000); printi(invert(0x8000000000000000)); }', b'-1\n-1\n9223372036854775807\n9223372036854775807\n', 0),
        ('bit_precedence', 'fn main() { printi(1 | 2 & 4); printi(1 ^ 3 & 1); printi(1 << 2+1); printi(16 >> 1+1); printi(1 | 2 && 0); printi((1 & 2)==2); printi(0b101010); printi(0B11); }', b'1\n0\n8\n4\n0\n0\n42\n3\n', 0),
        ('bit_evaluation', 'fn mark(x) { printi(x); return x; } fn main() { printi(mark(0) & mark(2)); printi(mark(1) | mark(3)); printi(mark(8) >>> mark(1)); }', b'0\n2\n0\n1\n3\n3\n8\n1\n4\n', 0),
        ('bitfields_example', (ROOT/'examples'/'bitfields.ae').read_text(encoding='utf-8'), b'3\n10\n42\n1\n', 0),
    ])
    all_values = ','.join(str(i) for i in range(256))
    all_reads = ''.join(f'printi(a[{i}]);' for i in range(256))
    cases.append(('array_full_frame', f'fn main() {{ let a=[{all_values}]; {all_reads} }}', ''.join(f'{i}\n' for i in range(256)).encode(), 0))
    data = list(range(16))
    statements = ['let a=[' + ','.join(map(str, data)) + '];']
    oracle_output = []
    for step in range(48):
        index, left, right = (step*7)%16, (step*3+1)%16, (step*5+2)%16
        factor = step+3
        statements.append(f'a[{index}]=a[{left}]*{factor}+a[{right}]; printi(a[{index}]);')
        data[index] = signed(data[left]*factor+data[right])
        oracle_output.append(data[index])
        if step%11 == 0:
            permutation = [(i+7)%16 for i in range(16)]
            statements.append('a=[' + ','.join(f'a[{i}]' for i in permutation) + '];')
            data = [data[i] for i in permutation]
    statements.extend(f'printi(a[{i}]);' for i in range(16))
    oracle_output.extend(data)
    cases.append(('array_oracle', 'fn main() {' + ''.join(statements) + '}', ''.join(f'{i}\n' for i in oracle_output).encode(), 0))
    params = ','.join(f'a{i}' for i in range(32))
    args = ','.join(str(i+1) for i in range(32))
    weighted = '+'.join(f'a{i}*{i+1}' for i in range(32))
    cases.append(('32_args', f'fn f({params}) {{ return {weighted}; }} fn main() {{ printi(f({args})); }}', b'11440\n', 0))
    unsigned_operations = {
        '+': lambda a,b: (a+b) & mask, '-': lambda a,b: (a-b) & mask,
        '*': lambda a,b: (a*b) & mask, '/': lambda a,b: a//b,
        '%': lambda a,b: a%b, '>>': lambda a,b: a >> (b & 63),
        '<': lambda a,b: int(a<b), '>': lambda a,b: int(a>b),
        '<=': lambda a,b: int(a<=b), '>=': lambda a,b: int(a>=b),
    }
    for index, (op, oracle) in enumerate(unsigned_operations.items()):
        result_type = 'bool' if op in ('<', '>', '<=', '>=') else 'u64'
        body, expected = [], []
        for a in [0, 1, 1 << 63, mask, 0x123456789ABCDEF0]:
            for b in [1, 2, 63, 64, mask]:
                body.extend([f'printi(calc({a},{b}));', f'printi(u64({a}) {op} u64({b}));'])
                expected.extend([str(oracle(a,b))] * 2)
        source = f'fn calc(a:u64,b:u64) -> {result_type} {{ return a {op} b; }} fn main() {{' + ''.join(body) + 'return 0; }'
        cases.append((f'unsigned_oracle_{index}', source, ('\n'.join(expected)+'\n').encode(), 0))
    cases.extend([
        ('typed_example', (ROOT/'examples'/'typed.ae').read_text(encoding='utf-8'), b'18446744073709551615\n1844674407370955161\n1\n1\n-1\n1\n', 0),
        ('boolean_runtime', 'fn convert(x:i64)->bool { return bool(x); } fn main() { printi(convert(-1)); printi(convert(0)); let x:bool=true; printi(!x); printi(i64(x)); printi(u64(x)); }', b'1\n0\n0\n1\n1\n', 0),
        ('unsigned_division_zero', 'fn f(x:u64,y:u64)->u64 { return x/y; } fn main() { printi(f(1,0)); }', b'', 70),
        ('typed_stack_args', 'fn f(a:u64,b:bool,c:i64,d:u64,e:bool,f:u64)->u64 { if(e && b) { return a+d+f+u64(c); } return 0; } fn main() { printi(f(0xffffffffffffffff,true,1,2,true,3)); }', b'5\n', 0),
    ])
    errors.append(('33_params', 'fn f(' + ','.join(f'a{i}' for i in range(33)) + ') {} fn main() {}', b'maximum 32 function parameters'))
    errors.append(('33_args', 'fn main() { return f(' + ','.join('0' for _ in range(33)) + '); }', b'maximum 32 function arguments'))
    errors.append(('live_slot_limit', 'fn main() {' + ''.join(f'let x{i}={i};' for i in range(257)) + '}', b'local variable limit'))
    errors.extend([
        ('mask_bool_precedence', 'fn main() { printi(1 & 2==2); }', b'type mismatch'),
        ('mixed_sign', 'fn main() { let x:u64=1; let y:i64=1; printi(x+y); }', b'type mismatch'),
        ('return_bool_as_int', 'fn main() { return true; }', b'type mismatch'),
        ('typed_call', 'fn f(x:bool) {} fn main() { f(1); }', b'type mismatch'),
        ('unknown_type', 'fn main() { let x:float=1; }', b'unknown type: float'),
    ])
    cases.extend([
        ('array_functions', (ROOT/'examples'/'array-functions.ae').read_text(encoding='utf-8'), b'28\n8\n21\n', 0),
        ('array_param_isolation', 'fn mutate(a:[i64;3],b:[i64;3]) { a[0]=90; b[1]=80; printi(a[1]); printi(b[0]); } fn main() { let x=[1,2,3]; mutate(x,x); printi(x[0]); printi(x[1]); }', b'2\n1\n1\n2\n', 0),
        ('array_literal_order', 'fn mark(x) { printi(x); return x; } fn f(a:[i64;3],b:i64,c:[i64;2]) { printi(a[0]*100+a[1]*10+a[2]); printi(b); printi(c[0]*10+c[1]); } fn main() { f([mark(1),mark(2),mark(3)],mark(4),[mark(5),mark(6)]); }', b'1\n2\n3\n4\n5\n6\n123\n4\n56\n', 0),
        ('array_mixed_stack_params', 'fn f(a:[i64;2],b:i64,c:[u64;2],d:bool,e:[bool;2],f:i64,g:[i64;1]) { printi(a[1]); printi(b); printi(c[0]); printi(d); printi(e[1]); printi(f); printi(g[0]); } fn main() { f([1,2],3,[0xffffffffffffffff,4],true,[false,true],6,[7]); }', b'2\n3\n18446744073709551615\n1\n1\n6\n7\n', 0),
        ('array_nested_calls', 'fn sum(a:[i64;2])->i64 { return a[0]+a[1]; } fn f(a:[i64;2],b:i64,c:[i64;2],d:i64,e:[i64;2])->i64 { return sum(a)+b+sum(c)+d+sum(e); } fn main() { printi(1+f([sum([1,2]),4],sum([5,6]),[7,sum([8,9])],sum([10,11]),[12,13])*2); }', b'177\n', 0),
        ('array_recursive_copy', 'fn count(n:i64,a:[i64;2])->i64 { if(n==0) { return a[0]+a[1]; } a[0]=a[0]+1; return count(n-1,a); } fn main() { let a=[1,2]; printi(count(30,a)); printi(a[0]); }', b'33\n1\n', 0),
        ('array_param_bounds', 'fn f(a:[i64;2]) { printi(a[-1]); } fn main() { f([1,2]); }', b'', 72),
        ('array_literal_bounds_order', 'fn mark(x) { printi(x); return x; } fn f(a:[i64;2]) {} fn main() { let a=[1,2]; f([mark(1),a[2]]); }', b'1\n', 72),
    ])
    one_array_params = ','.join(f'a{i}:[i64;1]' for i in range(32))
    one_array_args = ','.join(f'[{i+1}]' for i in range(32))
    array_weighted = '+'.join(f'a{i}[0]*{i+1}' for i in range(32))
    cases.append(('32_array_params', f'fn f({one_array_params}) {{ return {array_weighted}; }} fn main() {{ printi(f({one_array_args})); }}', b'11440\n', 0))
    cases.append(('full_array_param', f'fn f(a:[i64;256]) {{ printi(a[0]); printi(a[255]); printi(len(a)); a[0]=999; }} fn main() {{ let a=[{all_values}]; f(a); printi(a[0]); }}', b'0\n255\n256\n0\n', 0))
    cases.append(('full_array_literal_param', f'fn f(a:[i64;256]) {{ printi(a[0]); printi(a[127]); printi(a[255]); }} fn main() {{ f([{all_values}]); }}', b'0\n127\n255\n', 0))
    errors.extend([
        ('array_param_length', 'fn f(a:[i64;2]) {} fn main() { f([1]); }', b'type mismatch'),
        ('array_param_type', 'fn f(a:[u64;2]) {} fn main() { let a=[1,2]; f(a); }', b'type mismatch'),
        ('array_param_storage', 'fn f(a:[i64;256],b:i64) {} fn main() {}', b'local variable limit'),
    ])
    cases.extend([
        ('strings_example', (ROOT/'examples'/'strings.ae').read_text(encoding='utf-8'), 'Hello, Aegis\nAegis\n12\n65\n1\nUTF-8: 🙂\n'.encode(), 0),
        ('string_empty', 'fn empty()->str {} fn main() { print(empty()); printi(len(empty())); println(""); printi(""==empty()); }', b'0\n\n1\n', 0),
        ('string_binary_safe', r'fn main() { print("a\0b\n\t\r\\\""); printi(len("a\0b")); }', b'a\0b\n\t\r\\"3\n', 0),
        ('string_content_equality', r'fn same(a:str,b:str)->bool { return a==b; } fn main() { printi(same("a","a")); printi(same("a","aa")); printi(same("ab\0x","ab\0y")); printi(same("Я","\u{42F}")); printi("a"!="b"); }', b'1\n0\n0\n1\n1\n', 0),
        ('string_byte_indexing', 'fn main() { let s="Я🙂"; let i=0; while(i<len(s)) { printi(s[i]); i=i+1; } }', b'208\n175\n240\n159\n153\n130\n', 0),
        ('string_negative_index', 'fn main() { printi("abc"[-1]); }', b'', 72),
        ('string_past_end', 'fn main() { let s="abc"; printi(s[len(s)]); }', b'', 72),
        ('string_empty_index', 'fn main() { printi(""[0]); }', b'', 72),
        ('string_evaluation_order', 'fn source()->str { printi(1); return "ab"; } fn index() { printi(0); return 0; } fn main() { printi(source()[index()]); printi(len(source())); }', b'1\n0\n97\n1\n2\n', 0),
        ('string_array_return', 'fn first(a:[str;2])->str { a[1]="changed"; return a[0]; } fn main() { let a=["original","untouched"]; println(first(a)); println(a[1]); a[0]="new"; println(a[0]); }', b'original\nuntouched\nnew\n', 0),
        ('string_stack_args', 'fn select(a:str,b:i64,c:str,d:bool,e:str,f:str)->str { if(d) { return e; } return f; } fn main() { println(select("a",2,"c",true,"e","f")); println(select("a",2,"c",false,"e","f")); }', b'e\nf\n', 0),
        ('string_recursive_return', 'fn recurse(n:i64,s:str)->str { if(n==0) { return s; } return recurse(n-1,s); } fn main() { println(recurse(100,"retained")); printi(1+i64("a"=="a")+i64("x"!="y")); }', b'retained\n3\n', 0),
    ])
    long_string = 'Ж🙂x'*10000
    cases.append(('string_large_data', 'fn main() { print("'+long_string+'"); }', long_string.encode(), 0))
    errors.extend([
        ('string_immutable', 'fn main() { let s="x"; s[0]=1; }', b'immutable'),
        ('string_bad_escape', r'fn main() { println("\q"); }', b'unknown string escape'),
        ('string_bad_unicode', r'fn main() { println("\u{D800}"); }', b'invalid Unicode'),
        ('string_pointer_cast', 'fn main() { printi(u64("x")); }', b'found str'),
    ])
    cases.extend([
        ('aslr_workload', 'mod world { struct P { x:i64, name:str } fn make(n)->P { return P { x:n,name:"loaded" }; } fn sum(n) { if(n==0) { return 0; } return n+sum(n-1); } } fn main() { let p=world::make(21); let a=[p;2]; a[1].x=42; if(a[0].x!=21 || a[1].x!=42 || world::sum(9)!=45 || len(a[0].name)!=6 || a[0].name[1]!=111 || a[0].name!="loaded") { return 99; } println(a[0].name); printi(a[1].x); return 7; }', b'loaded\n42\n', 7),
        ('import_function', 'use math::twice as double; fn main() { printi(double(21)); } mod math { fn twice(n) { return n*2; } }', b'42\n', 0),
        ('import_module', 'mod math { mod inner { fn value() { return 42; } } } use math as m; fn main() { printi(m::inner::value()); }', b'42\n', 0),
        ('import_record', 'use model::P as Point; use model::make; mod model { struct P { x:i64 } fn make()->P { return P { x:42 }; } } fn take(p:Point)->Point { p.x=p.x+1; return p; } fn main() { let p:Point=make(); let q=take(p); printi(p.x); printi(q.x); }', b'42\n43\n', 0),
        ('import_scope', 'mod a { fn f() { return 1; } } mod b { fn f() { return 2; } } use a::f as f; mod app { use b::f as f; mod child { fn run() { return f(); } } fn run() { return child::run(); } } fn main() { printi(f()); printi(app::run()); }', b'1\n2\n', 0),
        ('import_absolute', 'fn f() { return 42; } mod a { struct P { x:i64 } } mod app { use a::P as f; fn run() { return ::f(); } } fn main() { printi(app::run()); }', b'42\n', 0),
        ('import_relative', 'mod world { mod math { fn score() { return 42; } } use math::score; mod app { use math as m; fn run() { return m::score()+score(); } } fn run() { return app::run(); } } fn main() { printi(world::run()); }', b'84\n', 0),
    ])
    errors.extend([
        ('import_unknown', 'use missing::f; fn main() {}', b'unknown import target'),
        ('import_conflict', 'mod a { fn f() {} } use a::f as main; fn main() {}', b'import name already defined'),
        ('import_wrong_kind', 'fn f() { return 99; } mod a { struct P { x:i64 } } mod app { use a::P as f; fn run() { return f(); } } fn main() {}', b'undefined function'),
        ('import_nominal', 'mod a { struct P { x:i64 } } mod b { struct P { x:i64 } } use a::P as A; use b::P as B; fn main() { let p:A=B { x:1 }; }', b'expected a::P'),
    ])
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
        # Observe the actual image base through documented debug events. These
        # are only freshly created fixture children; no process-memory access.
        for name in ['aslr_workload', 'string_large_data', 'return_full_frame',
                     'return_32_arguments', 'array_negative_read', 'division_zero']:
            exe = folder/(name+'.exe')
            data = exe.read_bytes()
            preferred = int.from_bytes(data[0x98+0x18:0x98+0x20], 'little')
            base, code = run_relocated(exe)
            expected_code = next(code for n, _, _, code in cases if n == name)
            assert base != preferred and base > 0xffffffff, (name, hex(base), hex(preferred))
            assert code == expected_code, (name, code, expected_code)
            print('PASS mandatory ASLR', name, hex(base))
        # Disabling DynamicBase on a fixture still has to rebase under the
        # mandatory child policy: this checks the harness really forces ASLR.
        data = bytearray((folder/'aslr_workload.exe').read_bytes())
        data[0x98+0x46:0x98+0x48] = (0x100).to_bytes(2, 'little')
        forced = folder/'mandatory-aslr.exe'
        forced.write_bytes(data)
        preferred = int.from_bytes(data[0x98+0x18:0x98+0x20], 'little')
        base, code = run_relocated(forced)
        assert base != preferred and code == 7, (hex(base), code)
        print('PASS mandatory policy rebases a non-DynamicBase fixture')
        # A fixed-image control must be refused when relocations are required.
        data[0x96:0x98] = (0x23).to_bytes(2, 'little')
        data[0x98+0x98:0x98+0xa0] = bytes(8)
        fixed = folder/'stripped-aslr.exe'
        fixed.write_bytes(data)
        try:
            run_relocated(fixed)
        except OSError as error:
            assert error.winerror in (193, 487, 623), error
        else:
            raise AssertionError('mandatory ASLR accepted a stripped fixed image')
        print('PASS mandatory ASLR rejects missing relocations')
        src, hanging = folder/'aslr-timeout.ae', folder/'aslr-timeout.exe'
        src.write_text('fn main() { while(1) {} }', encoding='utf-8')
        result = run(COMPILER, 'build', src, hanging)
        assert result.returncode == 0, result.stderr
        try:
            run_relocated(hanging, timeout=0.25)
        except TimeoutError:
            pass
        else:
            raise AssertionError('ASLR test failed to stop its own timed-out child')
        # The image must be released after cleanup, not left running/locked.
        hanging.unlink()
        print('PASS ASLR timeout cleans up its own child and image')
        project_folder=folder/'project'
        project_folder.mkdir()
        (project_folder/'world').mkdir()
        entry=project_folder/'main.ae'
        entry.write_text('mod world; use world::value as answer; fn main() { printi(answer()); }',encoding='utf-8')
        module=project_folder/'world.ae'
        module.write_text('mod math; use math::double; fn value() { return double(21); }',encoding='utf-8')
        child=project_folder/'world/math.ae'
        child.write_text('fn double(n) { return n*2; }',encoding='utf-8')
        project_exe=project_folder/'program.exe'
        result=run(COMPILER,'build',entry,project_exe)
        assert result.returncode==0,(result.stdout,result.stderr)
        assert run(project_exe).stdout==b'42\n'
        print('PASS native multi-file project')
        result=run(COMPILER,'check',entry)
        assert result.returncode==0,(result.stdout,result.stderr)
        print('PASS project check command')
        before=module.read_bytes()
        result=run(COMPILER,'build',entry,module)
        assert result.returncode==2 and module.read_bytes()==before
        print('PASS module output collision preserves source')
        child.write_text('// привет\nfn double(n) { return missing; }',encoding='utf-8')
        result=run(COMPILER,'check',entry)
        assert result.returncode==2 and b'ERROR 2:' in result.stderr and b'math.ae' in result.stderr,(result.stdout,result.stderr)
        print('PASS dependency diagnostic has file and original location')
        child.unlink()
        result=run(COMPILER,'check',entry)
        assert result.returncode==2 and b'cannot load module' in result.stderr
        print('PASS missing project dependency rejected')
        inspected = folder/'strings_example.exe'
        before = inspected.read_bytes()
        report = run(COMPILER, 'inspect', inspected)
        assert report.returncode == 0 and all(s in report.stdout for s in [b'PE64 x64',b'.rdata',b'.reloc',b'KERNEL32.DLL',b'WriteFile',b'NX-compatible=true',b'DynamicBase=true',b'HighEntropyVA=true']), (report.stdout,report.stderr)
        assert inspected.read_bytes() == before
        print('PASS read-only PE inspection')
        damaged = folder/'invalid-pe.bin'
        damaged.write_bytes(b'MZ'+b'\xff'*70)
        report = run(COMPILER,'inspect',damaged)
        assert report.returncode == 2 and b'PE:' in report.stderr and not report.stdout
        print('PASS malformed PE rejected')
        for bits in [32, 64]:
            exported = folder/f'exports{bits}.dll'
            fixture = export_fixture(bits)
            exported.write_bytes(fixture)
            report = run(COMPILER, 'inspect', exported)
            assert report.returncode == 0, report.stderr
            for expected in [f'PE{bits}'.encode(), b'Imports (1)', b'FIXTUREIMPORT.dll', b'external', b'Exports (4)', b'fixture.dll', b'ordinal #7', b'RVA 0x00002000', b'file 0xC00', b'alias', b'alpha', b'forwarder OTHER.#27', b'ordinal #10', b'no unique file mapping']:
                assert expected in report.stdout, (bits, expected, report.stdout)
            assert exported.read_bytes() == fixture
            print('PASS read-only export metadata', bits)
            fixture[0x544:0x546] = (5).to_bytes(2, 'little')
            exported.write_bytes(fixture)
            report = run(COMPILER, 'inspect', exported)
            assert report.returncode == 2 and b'invalid address-table slot' in report.stderr and not report.stdout
            assert exported.read_bytes() == fixture
            print('PASS invalid export ordinal rejected', bits)
        for name, source, diagnostic in errors:
            src, exe = folder / (name+'.ae'), folder / (name+'.exe')
            src.write_text(source, encoding='utf-8')
            result = run(COMPILER, 'build', src, exe)
            assert result.returncode == 2 and diagnostic in result.stdout + result.stderr and not exe.exists(), (name, result.stdout, result.stderr, result.returncode)
            print('PASS reject', name)
        result = run(COMPILER, 'check', ROOT/'examples'/'factorial.ae')
        assert result.returncode == 0 and b'FAIL' not in result.stdout, result.stdout
        print('PASS shared frontend check command')
        key, wrong_key = folder/'key.bin', folder/'wrong-key.bin'
        key.write_bytes(os.urandom(32))
        wrong_key.write_bytes(os.urandom(32))
        src, enc, dec, exe = (folder/n for n in ('protected.ae','protected.aeg','restored.ae','protected.exe'))
        (folder/'protected_helper.ae').write_text('fn value() { return 123; }',encoding='utf-8')
        src.write_bytes(b'mod protected_helper; fn main() { printi(protected_helper::value()); return 0; }')
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
    print(f'{len(cases) + len(errors) + 30} checks passed')


if __name__ == '__main__':
    main()
