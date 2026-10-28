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
    record_fields = ','.join(f'n{i}:i64' for i in range(256))
    record_values = ','.join(f'n{i}:{i}' for i in reversed(range(256)))
    cases.append(('struct_full_layout', 'struct P {'+record_fields+'} fn main() { let p=P {'+record_values+'}; printi(p.n0); printi(p.n255); p.n255=42; printi(p.n255); }', b'0\n255\n42\n', 0))
    cases.append(('struct_full_argument', 'struct P {'+record_fields+'} fn f(p:P) { printi(p.n0); printi(p.n255); p.n255=42; printi(p.n255); } fn main() { f(P {'+record_values+'}); }', b'0\n255\n42\n', 0))
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
    # Narrow integers (i8/u8..i32/u32): every result is checked against an
    # independent Python model of wrapping width semantics. Untyped literals
    # stay i64, so two-billion values never need a suffix by default.
    def make_signed(width):
        mask = (1 << width) - 1

        def f(n):
            n &= mask
            return n - (1 << width) if n >= (1 << (width - 1)) else n
        return f

    def make_unsigned(width):
        mask = (1 << width) - 1
        return lambda n: n & mask

    def tdiv(a, b):
        q = abs(a) // abs(b)
        return q if (a < 0) == (b < 0) else -q

    def tmod(a, b):
        return a - tdiv(a, b) * b

    i32_values = [0, 1, 2, 5, 31, 100, 0x12345678, 0x7fffffff,
                  -1, -2, -31, -100, -2147483648, 2147483647, -0x12345678]
    u32_values = [0, 1, 2, 5, 31, 100, 0x12345678, 0x7fffffff, 0x80000000, 4294967295]
    i16_values = [0, 1, 2, 5, 1000, 32767, -1, -2, -1000, -32768]
    u16_values = [0, 1, 2, 5, 1000, 32767, 40000, 65535]
    i8_values = [0, 1, 2, 5, 31, 100, 127, -1, -2, -31, -100, -128]
    u8_values = [0, 1, 2, 5, 31, 100, 127, 200, 255]

    def binary_oracle(name, ops, ty, result_ty, literal, values):
        for index, (op, model) in enumerate(ops.items()):
            body, expected = [], []
            for i, a in enumerate(values):
                for b in [values[(i + 3) % len(values)],
                          values[(i + 1) % len(values)],
                          values[len(values) - 1 - i]]:
                    la, lb = literal(a), literal(b)
                    body.extend([f'printi(calc({la},{lb}));', f'printi({ty}({la}) {op} {lb});'])
                    expected.extend([str(model(a, b))] * 2)
            source = (f'fn calc(a: {ty}, b: {ty}) -> {result_ty} {{ return a {op} b; }} '
                      'fn main() {' + ''.join(body) + 'return 0; }')
            cases.append((f'{name}_{index}', source, ('\n'.join(expected) + '\n').encode(), 0))

    def shift_oracle(name, ty, literal, values, models, counts):
        for index, (op, model) in enumerate(models.items()):
            body, expected = [], []
            for a in values:
                for k in counts:
                    la, lk = literal(a), str(k)
                    body.extend([f'printi(shift({la},{lk}));', f'printi({ty}({la}) {op} {lk});'])
                    expected.extend([str(model(a, k))] * 2)
            source = (f'fn shift(a: {ty}, k: i64) -> {ty} {{ return a {op} k; }} '
                      'fn main() {' + ''.join(body) + 'return 0; }')
            cases.append((f'{name}_{index}', source, ('\n'.join(expected) + '\n').encode(), 0))

    def division_oracle(name, ty, literal, values, quotient, remainder, minimum):
        body, expected = [], []
        for a in values:
            for b in values:
                if b == 0 or (minimum is not None and a == minimum and b == -1):
                    continue
                la, lb = literal(a), literal(b)
                body.extend([f'printi(q({la},{lb}));', f'printi({ty}({la}) / {lb});',
                             f'printi(r({la},{lb}));', f'printi({ty}({la}) % {lb});'])
                expected.extend([str(quotient(a, b)), str(quotient(a, b)),
                                 str(remainder(a, b)), str(remainder(a, b))])
        source = (f'fn q(a: {ty}, b: {ty}) -> {ty} {{ return a / b; }} '
                  f'fn r(a: {ty}, b: {ty}) -> {ty} {{ return a % b; }} '
                  'fn main() {' + ''.join(body) + 'return 0; }')
        cases.append((name, source, ('\n'.join(expected) + '\n').encode(), 0))

    def unary_oracle(name, ty, literal, values, model):
        body, expected = [], []
        for a in values:
            la = literal(a)
            body.extend([f'printi(neg({la}));', f'printi(inv({la}));'])
            expected.extend([str(model[0](a)), str(model[1](a))])
        source = (f'fn neg(a: {ty}) -> {ty} {{ return -a; }} '
                  f'fn inv(a: {ty}) -> {ty} {{ return ~a; }} '
                  'fn main() {' + ''.join(body) + 'return 0; }')
        cases.append((name, source, ('\n'.join(expected) + '\n').encode(), 0))

    for width, sname, uname, svalues, uvalues, minimum in [
        (32, 'i32', 'u32', i32_values, u32_values, -(1 << 31)),
        (16, 'i16', 'u16', i16_values, u16_values, -(1 << 15)),
        (8, 'i8', 'u8', i8_values, u8_values, -(1 << 7)),
    ]:
        signed, unsigned = make_signed(width), make_unsigned(width)
        slit = lambda n, s=signed: f'(-{-s(n)})' if s(n) < 0 else str(s(n))
        ulit = lambda n, u=unsigned: str(u(n))
        counts = [0, 1, 5, width - 1, width, width + 1, 63, 0xffffffff]
        binary_oracle(f'int{width}_arith', {
            '+': lambda a, b, s=signed: s(a + b), '-': lambda a, b, s=signed: s(a - b),
            '*': lambda a, b, s=signed: s(a * b), '&': lambda a, b, s=signed: s(a & b),
            '|': lambda a, b, s=signed: s(a | b), '^': lambda a, b, s=signed: s(a ^ b),
        }, sname, sname, slit, svalues)
        binary_oracle(f'int{width}_cmp', {
            '==': lambda a, b: int(a == b), '!=': lambda a, b: int(a != b),
            '<': lambda a, b: int(a < b), '>': lambda a, b: int(a > b),
            '<=': lambda a, b: int(a <= b), '>=': lambda a, b: int(a >= b),
        }, sname, 'bool', slit, svalues)
        binary_oracle(f'uint{width}_arith', {
            '+': lambda a, b, u=unsigned: u(a + b), '-': lambda a, b, u=unsigned: u(a - b),
            '*': lambda a, b, u=unsigned: u(a * b), '&': lambda a, b: a & b,
            '|': lambda a, b: a | b, '^': lambda a, b: a ^ b,
        }, uname, uname, ulit, uvalues)
        binary_oracle(f'uint{width}_cmp', {
            '==': lambda a, b: int(a == b), '!=': lambda a, b: int(a != b),
            '<': lambda a, b: int(a < b), '>': lambda a, b: int(a > b),
        }, uname, 'bool', ulit, uvalues)
        shift_oracle(f'int{width}_shift', sname, slit, svalues, {
            '<<': lambda a, k, s=signed, m=width - 1: s(a << (k & m)),
            '>>': lambda a, k, m=width - 1: a >> (k & m),
            '>>>': lambda a, k, s=signed, u=unsigned, m=width - 1: s(u(a) >> (k & m)),
        }, counts)
        shift_oracle(f'uint{width}_shift', uname, ulit, uvalues, {
            '<<': lambda a, k, u=unsigned, m=width - 1: u(a << (k & m)),
            '>>': lambda a, k, m=width - 1: a >> (k & m),
            '>>>': lambda a, k, m=width - 1: a >> (k & m),
        }, counts)
        division_oracle(f'int{width}_division', sname, slit, svalues, tdiv, tmod, minimum)
        division_oracle(f'uint{width}_division', uname, ulit, uvalues,
                        lambda a, b: a // b, lambda a, b: a % b, None)
        unary_oracle(f'int{width}_unary', sname, slit, svalues,
                     (lambda a, s=signed: s(-a), lambda a, s=signed: s(~a)))
        unary_oracle(f'uint{width}_unary', uname, ulit, uvalues,
                     (lambda a, u=unsigned: u(-a), lambda a, u=unsigned: u(~a)))
    # f64: IEEE-754 doubles checked against Python's native float model.
    f64_values = [0.0, 1.0, -1.0, 0.5, -0.5, 2.5, -2.5, 0.25, 3.75, -0.125,
                  123.5, 100000.0, -100000.0, 0.0009765625, 1e10, -0.001]

    def fdiv(a, b):
        try:
            return a / b
        except ZeroDivisionError:
            if a == 0 or a != a:
                return float('nan')
            return float('inf') if a > 0 else float('-inf')

    def cvt_i64(v):
        if v != v or v >= 2 ** 63 or v < -(2 ** 63):
            return -(2 ** 63)
        return int(v)

    def cvt_u64(v):
        return int(v) & 0xFFFFFFFFFFFFFFFF

    def ffmt(v):
        # Mirror of the printd algorithm: f64 scaling with round-half-up on
        # the fraction, nan/inf spellings, values at or above 2^63 as inf.
        # The last digit may differ from ideal decimal rounding of the exact
        # binary value; that is a documented property of the helper.
        import math
        if v != v:
            return 'nan'
        if v == float('inf'):
            return 'inf'
        if v == float('-inf'):
            return '-inf'
        sign = '-' if v < 0 or (v == 0 and math.copysign(1.0, v) < 0) else ''
        a = abs(v)
        if a >= 2 ** 63:
            return sign + 'inf'
        whole = int(a)
        d = int((a - whole) * 1e6 + 0.5)
        if d >= 1000000:
            d -= 1000000
            whole += 1
        return f'{sign}{whole}.{d:06d}'

    def float_binary_oracle(name, ops, result_ty):
        for index, (op, model) in enumerate(ops.items()):
            body, expected = [], []
            for i, a in enumerate(f64_values):
                for b in [f64_values[(i + 3) % len(f64_values)],
                          f64_values[(i + 1) % len(f64_values)],
                          f64_values[len(f64_values) - 1 - i]]:
                    la, lb = repr(a), repr(b)
                    body.extend([f'printi(calc({la},{lb}));',
                                 f'printi(f64({la}) {op} f64({lb}));'])
                    value = model(a, b)
                    text = ffmt(value) if result_ty == 'f64' else str(int(value))
                    expected.extend([text] * 2)
            source = (f'fn calc(a: f64, b: f64) -> {result_ty} {{ return a {op} b; }} '
                      'fn main() {' + ''.join(body) + 'return 0; }')
            cases.append((f'{name}_{index}', source, ('\n'.join(expected) + '\n').encode(), 0))

    float_binary_oracle('f64_arith', {
        '+': lambda a, b: a + b, '-': lambda a, b: a - b,
        '*': lambda a, b: a * b, '/': fdiv,
    }, 'f64')
    float_binary_oracle('f64_cmp', {
        '==': lambda a, b: a == b, '!=': lambda a, b: a != b,
        '<': lambda a, b: a < b, '>': lambda a, b: a > b,
        '<=': lambda a, b: a <= b, '>=': lambda a, b: a >= b,
    }, 'bool')

    def float_unary_oracle():
        body, expected = [], []
        for v in f64_values:
            lv = repr(v)
            body.extend([f'printi(neg({lv}));', f'printi(anti({lv}));'])
            expected.extend([ffmt(-v), '1' if not (v == v and v != 0.0) else '0'])
        source = ('fn neg(a: f64) -> f64 { return -a; } '
                  'fn anti(a: f64) -> bool { return !a; } '
                  'fn main() {' + ''.join(body) + 'return 0; }')
        cases.append(('f64_unary', source, ('\n'.join(expected) + '\n').encode(), 0))

    float_unary_oracle()

    def float_conversion_oracle():
        body, expected = [], []
        for v in [3.99, -3.99, 0.5, -0.5, 2147483648.0, -2147483648.0, 1e10, -1e10, 0.0]:
            lv = repr(v)
            i, u = cvt_i64(v), cvt_u64(v)
            body.extend([f'printi(i64({lv}));', f'printi(u64({lv}));', f'printi(f64(i64({lv})));'])
            expected.extend([str(i), str(u), ffmt(float(i))])
        source = 'fn main() {' + ''.join(body) + 'return 0; }'
        cases.append(('f64_conversions', source, ('\n'.join(expected) + '\n').encode(), 0))

    float_conversion_oracle()

    cases.extend([
        ('f64_u64_runtime_split', 'fn t(v: f64) -> u64 { return u64(v); } fn main() { printi(t(1e19)); printi(t(1e300)); printi(t(3.99)); printi(t(-1.5)); }', b'10000000000000000000\n0\n3\n18446744073709551615\n', 0),
        ('f64_u64_edges', 'fn main() { printi(u64(1e19)); printi(u64(0.0/0.0)); printi(u64(1e300)); printi(u64(-1.5)); }', b'10000000000000000000\n9223372036854775808\n0\n18446744073709551615\n', 0),
        ('f64_i64_indefinite', 'fn main() { printi(i64(1e300)); printi(i64(0.0/0.0)); }', b'-9223372036854775808\n-9223372036854775808\n', 0),
        ('f64_abi_mixed', 'fn mix(a: f64, b: i64, c: f64, d: str, e: f64, f: i64) -> f64 { return a + f64(b) + c + f64(len(d)) + e + f64(f); } fn main() { printi(mix(1.5, 10, 0.25, "abcd", -2.0, 100)); }', b'113.750000\n', 0),
        ('f64_aggregates', 'struct P { x: f64, y: f64 } fn sum(p: P) -> f64 { return p.x + p.y; } fn main() { let a = P { x: 1.5, y: 2.25 }; let b = a; b.x = 9.5; printi(sum(a)); printi(sum(b)); let arr: [f64;3] = [0.5, 1.5, 2.5]; printi(arr[0] + arr[1] + arr[2]); }', b'3.750000\n11.750000\n4.500000\n', 0),
        ('f64_recursion', 'fn half(n: f64) -> f64 { if (n < 1.0) { return n; } return half(n / 2.0); } fn main() { printi(half(100.0)); }', b'0.781250\n', 0),
        ('f64_zero_sign', 'fn main() { printi(-0.0); printi(0.0); printi(-0.0 == 0.0); printi(1.0 / -0.0); }', b'-0.000000\n0.000000\n1\n-inf\n', 0),
        ('f64_short_circuit', 'fn main() { printi(0.0/0.0 && 1.0); printi(1.5 && 2.5); printi(0.0 || 1.5); }', b'0\n1\n1\n', 0),
    ])
    # f32: single precision checked against a Python f32 model built with
    # struct (round the inputs, compute in double, round the result).
    import struct as _struct

    def f32(v):
        return _struct.unpack('<f', _struct.pack('<f', v))[0]

    f32_values = [0.0, 1.0, -1.0, 0.5, -0.5, 2.5, -2.5, 0.25, 3.75, -0.125,
                  100.0, -100.0]

    def f32div(a, b):
        try:
            return f32(a / b)
        except ZeroDivisionError:
            if a == 0 or a != a:
                return float('nan')
            return float('inf') if a > 0 else float('-inf')

    def cvt_i64_32(v):
        if v != v or v >= 2 ** 63 or v < -(2 ** 63):
            return -(2 ** 63)
        return int(v)

    def float32_binary_oracle(name, ops, result_ty):
        for index, (op, model) in enumerate(ops.items()):
            body, expected = [], []
            for i, a in enumerate(f32_values):
                for b in [f32_values[(i + 3) % len(f32_values)],
                          f32_values[(i + 1) % len(f32_values)],
                          f32_values[len(f32_values) - 1 - i]]:
                    la, lb = repr(a), repr(b)
                    body.extend([f'printi(calc({la},{lb}));',
                                 f'printi(f32({la}) {op} f32({lb}));'])
                    value = model(f32(a), f32(b))
                    text = ffmt(value) if result_ty == 'f32' else str(int(value))
                    expected.extend([text] * 2)
            source = (f'fn calc(a: f32, b: f32) -> {result_ty} {{ return a {op} b; }} '
                      'fn main() {' + ''.join(body) + 'return 0; }')
            cases.append((f'{name}_{index}', source, ('\n'.join(expected) + '\n').encode(), 0))

    float32_binary_oracle('f32_arith', {
        '+': lambda a, b: f32(a + b), '-': lambda a, b: f32(a - b),
        '*': lambda a, b: f32(a * b), '/': f32div,
    }, 'f32')
    float32_binary_oracle('f32_cmp', {
        '==': lambda a, b: a == b, '!=': lambda a, b: a != b,
        '<': lambda a, b: a < b, '>': lambda a, b: a > b,
    }, 'bool')

    def float32_conversion_oracle():
        body, expected = [], []
        for v in [3.99, -3.99, 0.5, -0.5, 100.0, -100.0, 0.0]:
            lv = repr(v)
            f = f32(v)
            i, u = cvt_i64_32(f), int(f) & 0xFFFFFFFF
            body.extend([f'printi(i32({lv}));', f'printi(u32({lv}));',
                         f'printi(f64(f32({lv})));', f'printi(f32(f64({lv})));'])
            expected.extend([str(i), str(u), ffmt(float(f)), ffmt(f)])
        source = 'fn main() {' + ''.join(body) + 'return 0; }'
        cases.append(('f32_conversions', source, ('\n'.join(expected) + '\n').encode(), 0))

    float32_conversion_oracle()

    cases.extend([
        ('f32_abi_mixed', 'fn mix(a: f32, b: i64, c: f64, d: f32) -> f64 { return f64(a) + f64(b) + c + f64(d); } fn main() { printi(mix(1.5, 10, 0.25, -2.0)); }', b'9.750000\n', 0),
        ('f32_aggregates', 'struct P { x: f32, y: f32 } fn sum(p: P) -> f32 { return p.x + p.y; } fn main() { let a = P { x: 1.5, y: 2.25 }; let b = a; b.x = 9.5; printi(sum(a)); printi(sum(b)); let arr: [f32;2] = [0.5, 1.5]; printi(arr[0] + arr[1]); }', b'3.750000\n11.750000\n2.000000\n', 0),
        ('f32_u32_edges', 'fn main() { printi(u32(1e10)); printi(u32(0.0/0.0)); printi(u32(-1.5)); printi(i32(1e300)); }', b'1410065408\n0\n4294967295\n0\n', 0),
        ('f32_precision', 'fn main() { let a: f32 = 0.1; printi(a); printi(f64(a)); let x: f32 = 16777216.0; printi(x + 1.0); }', b'0.100000\n0.100000\n16777216.000000\n', 0),
    ])
    errors.extend([
        ('f32_strict_int', 'fn main() { let x: f32 = 2; }', b'type mismatch'),
        ('f32_strict_f64_mix', 'fn main() { let a: f32 = 1.5; let b = 2.5; printi(a + b); }', b'type mismatch'),
        ('f32_strict_f64', 'fn main() { let a: f32 = 1.0; let b: f64 = 1.0; printi(a + b); }', b'type mismatch'),
        ('f32_bitwise', 'fn main() { printi(1.5 & 2.0); }', b'require integers'),
    ])

    cases.extend([
        ('heap_concat_basics', 'fn main() { let who = "world"; let m = "hello, " + who + "!"; println(m); printi(len(m)); printi(m[7]); println("hello, " + "world"); }', b'hello, world!\n13\n119\nhello, world\n', 0),
        ('heap_concat_equality', 'fn main() { let m = "hello" + ", " + "world"; printi(m == "hello, world"); printi(m != "hello, world"); printi(m == "hello, world!"); let n = "hell" + "o"; printi(n == "hello"); }', b'1\n0\n0\n1\n', 0),
        ('heap_concat_stress', 'fn main() { let i = 0; let acc = ""; while(i < 20000) { acc = acc + "x"; i = i + 1; } printi(len(acc)); printi(acc[19999]); }', b'20000\n120\n', 0),
        ('heap_concat_functions', 'fn wrap(s: str) -> str { return "[" + s + "]"; } fn shout(s: str) -> str { return s + "!"; } fn main() { println(wrap(shout("ok"))); println(shout(wrap("a") + wrap("b"))); let nested = wrap(wrap("deep")); println(nested); }', b'[ok!]\n[a][b]!\n[[deep]]\n', 0),
        ('heap_concat_recursion', 'fn build(n: i64) -> str { if(n == 0) { return ""; } return build(n - 1) + "ab"; } fn main() { let s = build(10); printi(len(s)); println(s); }', b'20\nabababababababababab\n', 0),
        ('heap_concat_struct_fields', 'struct Named { label: str, id: i64 } fn label(n: Named) -> str { return n.label + ":"; } fn main() { let n = Named { label: "item", id: 7 }; println(label(n)); let deep = label(n) + label(n); println(deep); printi(n.id); }', b'item:\nitem:item:\n7\n', 0),
        ('heap_concat_arrays', 'fn main() { let words: [str;3] = ["x", "y", "z"]; let joined = words[0] + words[1] + words[2]; println(joined); let a: [str;2] = [joined;2]; println(a[1] + "!"); printi(len(a[0])); }', b'xyz\nxyz!\n3\n', 0),
        ('heap_concat_branches', 'fn pick(flag: bool) -> str { if(flag) { return "yes" + "!"; } return "no"; } fn main() { println(pick(true)); println(pick(false)); let i = 0; while(i < 3) { println(pick(i % 2 == 1)); i = i + 1; } }', b'yes!\nno\nno\nyes!\nno\n', 0),
    ])
    cases.extend([
        ('heap_transfer_field', 'struct Named { label: str, id: i64 } fn main() { let n = Named { label: "first", id: 1 }; n.label = "second" + "!"; println(n.label); printi(n.id); n.label = "third"; println(n.label); }', b'second!\n1\nthird\n', 0),
        ('heap_transfer_element', 'fn main() { let words: [str;2] = ["a", "b"]; words[0] = words[1] + "c"; println(words[0]); println(words[1]); words[1] = "z" + "z"; println(words[1]); }', b'bc\nb\nzz\n', 0),
        ('heap_transfer_struct_value', 'struct Named { label: str, id: i64 } fn main() { let n = Named { label: "first", id: 1 }; n = Named { label: "second" + "!", id: 2 }; println(n.label); printi(n.id); }', b'second!\n2\n', 0),
        ('heap_transfer_stress', 'struct Named { label: str, id: i64 } fn main() { let n = Named { label: "x", id: 0 }; let words: [str;2] = ["a", "b"]; let i = 0; while(i < 10000) { n.label = "s" + "t"; words[0] = "u" + "v"; i = i + 1; } printi(len(n.label)); println(words[0]); }', b'2\nuv\n', 0),
        ('heap_break_continue', 'struct Named { label: str, id: i64 } fn main() { let n = Named { label: "x", id: 0 }; let words: [str;2] = ["a", "b"]; let i = 0; while(i < 5) { n.label = "s" + "t"; words[1] = "u" + "v"; if(i == 2) { break; } if(i == 0) { i = i + 1; continue; } i = i + 1; } printi(i); println(n.label); println(words[1]); }', b'2\nst\nuv\n', 0),
        ('heap_transfer_aggregate_element', 'struct P { label: str, id: i64 } fn main() { let a: [P;2] = [P { label: "one", id: 1 };2]; a[1] = P { label: "two" + "!", id: 2 }; println(a[0].label); println(a[1].label); printi(a[1].id); }', b'one\ntwo!\n2\n', 0),
    ])

    errors.extend([
        ('heap_str_strict_arith', 'fn main() { printi("a" + 1); }', b'type mismatch'),
        ('heap_str_strict_cmp_ord', 'fn main() { printi("a" < "b"); }', b'strings support only'),
    ])

    cases.extend([
        ('dyn_basics', 'fn main() { let a: [i64] = [10, 20, 30]; printi(len(a)); printi(a[0]); printi(a[2]); let b = push(a, 40); printi(len(b)); printi(b[3]); printi(len(a)); printi(a[3]); }', b'3\n10\n30\n4\n40\n3\n', 72),
        ('dyn_value_semantics', 'fn main() { let c: [i64] = []; printi(len(c)); let d = push(c, 5); printi(len(c)); printi(len(d)); printi(d[0]); }', b'0\n0\n1\n5\n', 0),
        ('dyn_str_elements', 'fn main() { let w: [str] = ["alpha", "beta"]; let v = push(w, "gamma" + "!"); printi(len(w)); printi(len(v)); println(v[0]); println(v[2]); println(w[0]); }', b'2\n3\nalpha\ngamma!\nalpha\n', 0),
        ('dyn_element_assign', 'fn main() { let a: [i64] = [1, 2, 3]; a[1] = 20 + 2; printi(a[1]); let w: [str] = ["x"]; w[0] = "new" + "!"; println(w[0]); }', b'22\nnew!\n', 0),
        ('dyn_element_bounds', 'fn main() { let a: [i64] = [1]; printi(a[1]); }', b'', 72),
        ('dyn_negative_index', 'fn main() { let a: [i64] = [1]; printi(a[-1]); }', b'', 72),
        ('dyn_params_returns', 'fn total(v: [i64]) -> i64 { let i = 0; let s = 0; while(i < len(v)) { s = s + v[i]; i = i + 1; } return s; } fn add(v: [i64], x: i64) -> [i64] { return push(v, x); } fn main() { let v: [i64] = [10, 20, 30]; printi(total(v)); let v2 = add(v, 40); printi(len(v2)); printi(total(v2)); printi(len(v)); }', b'60\n4\n100\n3\n', 0),
        ('dyn_build_loop', 'fn squares(n: i64) -> [i64] { let v: [i64] = []; let i = 0; while(i < n) { v = push(v, i * i); i = i + 1; } return v; } fn main() { let s = squares(6); printi(len(s)); printi(s[4]); printi(s[5]); }', b'6\n16\n25\n', 0),
        ('dyn_str_build', 'fn join(n: i64) -> [str] { let v: [str] = []; let i = 0; while(i < n) { v = push(v, "s" + "x"); i = i + 1; } return v; } fn main() { let v = join(3); printi(len(v)); println(v[2]); let w = push(v, "tail"); printi(len(w)); println(w[3]); printi(len(v[0])); }', b'3\nsx\n4\ntail\n2\n', 0),
        ('dyn_in_struct_field', 'struct Bag { items: [i64], tag: str } fn main() { let b = Bag { items: [1, 2], tag: "bag" }; printi(len(b.items)); printi(b.items[1]); let b2 = Bag { items: push(b.items, 3), tag: "b2" }; printi(len(b2.items)); printi(b2.items[2]); printi(len(b.items)); }', b'2\n2\n3\n3\n2\n', 0),
        ('dyn_i32_u64_elements', 'fn main() { let a: [u32] = [4000000000, 1]; printi(a[0] + 1); let b: [u64] = [18000000000]; printi(b[0]); let c: [i32] = [-5]; printi(c[0]); }', b'4000000001\n18000000000\n-5\n', 0),
        ('dyn_repeat_empty', 'fn main() { let a: [i64] = [0; 4]; printi(len(a)); printi(a[3]); let e: [i64] = []; printi(len(e)); }', b'4\n0\n0\n', 0),
        ('dyn_recursion', 'fn sum_to(v: [i64], i: i64) -> i64 { if(i < 0) { return 0; } return v[i] + sum_to(v, i - 1); } fn main() { let v: [i64] = [5, 10, 15]; printi(sum_to(v, 2)); }', b'30\n', 0),
        ('dyn_field_mutation', 'struct Bag { items: [i64] } fn main() { let b = Bag { items: [1, 2] }; let copy = b; b.items[0] = 9; printi(b.items[0]); printi(copy.items[0]); b.items = push(b.items, 3); printi(len(b.items)); printi(len(copy.items)); }', b'9\n1\n3\n2\n', 0),
        ('dyn_float_elements', 'fn main() { let a: [f64] = [1.25, -2.5]; a[0] = a[1] * 2.0; let b = push(a, 3.75); printi(a[0]); printi(b[2]); let c: [f32] = [f32(1.5); 3]; c[1] = f32(-4.25); let d = push(c, f32(6.5)); printi(c[0]); printi(c[1]); printi(d[3]); }', b'-5.000000\n3.750000\n1.500000\n-4.250000\n6.500000\n', 0),
        ('dyn_string_repeat', 'fn make() -> [str] { return ["hello" + "!"; 4]; } fn main() { let a = make(); let b = a; a[1] = "new" + "!"; println(a[1]); println(b[1]); println(make()[3]); printi(len(make())); }', b'new!\nhello!\nhello!\n4\n', 0),
        ('dyn_temporary_index', 'fn make() -> [i64] { return [4, 5]; } fn main() { printi(push(make(), 6)[2]); printi(make()[1]); }', b'6\n5\n', 0),
        ('dyn_heap_stress', 'fn make() -> [str] { return ["one" + "!", "two" + "!"]; } fn main() { let i = 0; let saved = ""; while(i < 2000) { let a = make(); let b = a; a[0] = "replace" + "!"; let c = push(a, b[0]); saved = c[2]; a; push(c, "discard" + "!"); len(make()); i = i + 1; } println(saved); }', b'one!\n', 0),
        ('dyn_fixed_array_slots', 'fn main() { let x: [i64] = [1, 2]; let a = [x, x]; let b = a; a[0][1] = 99; printi(a[0][1]); printi(a[1][1]); printi(b[0][1]); }', b'99\n2\n2\n', 0),
        ('dyn_string_field_copy', 'struct Bag { items: [str], label: str } fn main() { let b = Bag { items: ["a" + "b"], label: "x" + "y" }; let c = b; b.items[0] = "c" + "d"; println(c.items[0]); println(b.items[0]); b.items = push(c.items, "tail" + "!"); println(b.items[1]); println(c.label); }', b'ab\ncd\ntail!\nxy\n', 0),
        ('dyn_float_return_cleanup', 'struct Bag { items: [str], label: str } fn sum(b: Bag) -> f64 { return 1.25; } fn test() -> f64 { let a: [str] = ["a" + "b"]; return 2.5; } fn main() { printi(sum(Bag { items: ["one" + "!"], label: "two" + "!" })); printi(test()); }', b'1.250000\n2.500000\n', 0),
        ('dyn_set_negative', 'fn main() { let a: [i64] = [1]; a[-1] = 2; }', b'', 72),
        ('dyn_set_empty', 'fn main() { let a: [str] = []; a[0] = "oops" + "!"; }', b'', 72),
        ('dyn_default_return', 'fn make() -> [str] {} fn main() { let a = make(); printi(len(a)); println(push(make(), "x" + "y")[0]); }', b'0\nxy\n', 0),
        ('dyn_runtime_limit', 'fn main() { let a: [i64] = [0; 1048576]; printi(len(a)); let b = push(a, 1); }', b'1048576\n', 73),
        ('dyn_large_repeat_once', 'fn item() -> str { printi(7); return "x" + "y"; } fn main() { let a: [str] = [item(); 20000]; println(a[19999]); printi(len(a)); }', b'7\nxy\n20000\n', 0),
        ('heap_allocation_limit', 'fn main() { let s = "x"; let i = 0; while(i < 30) { s = s + s; i = i + 1; } }', b'', 73),
        ('dyn_unique_set_stress', 'fn main() { let a: [i64] = [0; 100000]; let words: [str] = ["x" + "y", "a" + "b"]; let i = 0; while(i < 100000) { a[i] = i; words[0] = words[1]; i = i + 1; } printi(a[99999]); println(words[0]); }', b'99999\nab\n', 0),
        ('dyn_grow_million', 'fn main() { let a: [i64] = []; let i = 0; while(i < 1048576) { a = push(a, i); i = i + 1; } printi(len(a)); printi(a[0]); printi(a[65536]); printi(a[1048575]); a = push(a, 9); }', b'1048576\n0\n65536\n1048575\n', 73),
        ('dyn_grow_boundaries', 'fn main() { let a: [i64] = []; let i = 0; while(i < 65537) { a = push(a, i * 3); i = i + 1; } let sum = 0; i = 0; while(i < len(a)) { sum = sum + a[i]; i = i + 1; } printi(len(a)); printi(a[1]); printi(a[2]); printi(a[3]); printi(a[7]); printi(a[8]); printi(a[65536]); printi(sum); }', b'65537\n3\n6\n9\n21\n24\n196608\n6442549248\n', 0),
        ('dyn_grow_alias_with_capacity', 'fn main() { let a: [i64] = [10, 20, 30]; let b = a; a = push(a, 40); b[0] = 99; printi(a[0]); printi(a[3]); printi(len(b)); printi(b[0]); }', b'10\n40\n3\n99\n', 0),
        ('dyn_grow_string_alias_drop', 'fn main() { let a: [str] = ["a" + "!", "b" + "!", "c" + "!"]; let b = a; a = push(a, a[0]); b = []; let i = 0; while(i < 1000) { let noise = "reuse" + "heap"; i = i + 1; } println(a[0]); println(a[2]); println(a[3]); a = push(a, a[3]); println(a[4]); }', b'a!\nc!\na!\na!\n', 0),
        ('dyn_grow_string_unique', 'fn main() { let a: [str] = []; let i = 0; while(i < 20000) { a = push(a, "hello" + "!"); i = i + 1; } println(a[0]); println(a[19999]); printi(len(a)); let b = a; a = push(a, "tail" + "!"); println(b[19999]); println(a[20000]); printi(len(b)); }', b'hello!\nhello!\n20000\nhello!\ntail!\n20000\n', 0),
        ('dyn_grow_rhs_order', 'fn element(a: [i64]) -> i64 { printi(len(a)); return a[0] + len(a); } fn main() { let a: [i64] = [10]; a = push(a, element(a)); a = push(a, element(a)); printi(a[1]); printi(a[2]); }', b'1\n2\n11\n12\n', 0),
        ('dyn_grow_nested_temporary', 'fn values() -> [str] { return ["one" + "!"]; } fn main() { let a = push(push(push(values(), "two" + "!"), "three" + "!"), "four" + "!"); println(a[0]); println(a[1]); println(a[2]); println(a[3]); }', b'one!\ntwo!\nthree!\nfour!\n', 0),
        ('dyn_grow_float_bits', 'fn main() { let a: [f32] = []; let b: [f64] = []; let i = 0; while(i < 1000) { a = push(a, f32(i) * f32(0.5)); b = push(b, f64(i) * 0.25); i = i + 1; } let c = a; a = push(a, f32(-0.0)); printi(c[999]); printi(b[999]); printi(a[1000]); printi(len(c)); }', b'499.500000\n249.750000\n-0.000000\n1000\n', 0),
        ('dyn_grow_nested_field', 'struct Bag { items: [i64] } struct Outer { bag: Bag } fn main() { let b = Outer { bag: Bag { items: [] } }; let i = 0; while(i < 65537) { b.bag.items = push(b.bag.items, i); i = i + 1; } let c = b; b.bag.items = push(b.bag.items, b.bag.items[0]); printi(len(b.bag.items)); printi(len(c.bag.items)); printi(c.bag.items[65536]); }', b'65538\n65537\n65536\n', 0),
        ('dyn_grow_string_field', 'struct Bag { items: [str] } fn main() { let b = Bag { items: [] }; let i = 0; while(i < 1000) { b.items = push(b.items, "s" + "!"); i = i + 1; } let copy = b; b.items = push(b.items, b.items[0]); println(b.items[1000]); println(copy.items[999]); printi(len(copy.items)); }', b's!\ns!\n1000\n', 0),
        ('dyn_grow_fixed_slot_general', 'fn pick() -> i64 { printi(7); return 0; } fn main() { let x: [i64] = [1]; let a = [x]; a[pick()] = push(a[pick()], 2); printi(a[0][1]); printi(len(x)); }', b'7\n7\n2\n1\n', 0),
        ('dyn_grow_snapshot_per_iteration', 'fn main() { let a: [str] = []; let last: [str] = []; let i = 0; while(i < 1000) { last = a; a = push(a, "x" + "y"); i = i + 1; } printi(len(last)); printi(len(a)); println(last[998]); println(a[999]); }', b'999\n1000\nxy\nxy\n', 0),
        ('abi_default_f32_after_value', 'fn zero() -> f32 {} fn keep(x:f32) -> f32 { return x; } fn main() { let x = keep(7.25); printi(zero()); printi(x); }', b'0.000000\n7.250000\n', 0),
        ('abi_mixed_owned_arguments', 'fn mix(a:str,b:f64,c:[str],d:f32,e:i8)->f64 { println(a); println(c[0]); return b+f64(d)+f64(e); } fn main() { let values:[str] = ["array"+"!"]; printi(mix("text"+"!",2.5,values,f32(1.25),-2)); println(values[0]); }', b'text!\narray!\n1.750000\narray!\n', 0),
        ('abi_mixed_aggregate_return', 'struct Box { label:str, x:f64 } fn make(a:i64,b:f64,c:i64,d:f32,e:str)->Box { return Box { label:e, x:f64(a)+b+f64(c)+f64(d) }; } fn main() { let b = make(1,2.5,3,f32(4.25),"box"+"!"); println(b.label); printi(b.x); }', b'box!\n10.750000\n', 0),
        ('abi_mixed_aggregate_argument', 'struct Box { first:str, second:[str], x:f64 } fn sum(a:Box,b:f64,c:i64,d:f32,e:Box)->f64 { println(a.first); println(e.second[0]); return a.x+b+f64(c)+f64(d)+e.x; } fn main() { let a = Box { first:"one"+"!",second:["two"+"!"],x:1.25 }; printi(sum(a,2.5,3,f32(4.25),a)); println(a.second[0]); }', b'one!\ntwo!\n12.250000\ntwo!\n', 0),
    ])
    errors.extend([
        ('dyn_nested_element', 'fn main() { let a: [[i64]] = []; }', b'must be one-slot scalars or str'),
        ('dyn_temporary_assignment', 'fn make() -> [i64] { return [1]; } fn main() { make()[0] = 2; }', b'assignment requires a named array'),
        ('dyn_push_fixed', 'fn main() { let a = [1, 2]; printi(push(a, 3)); }', b'push requires a dynamic array'),
    ])

    errors.extend([
        ('float_strict_int_literal', 'fn main() { let x: f64 = 2; }', b'type mismatch'),
        ('float_strict_mixed_arith', 'fn main() { printi(1 + 1.5); }', b'type mismatch'),
        ('float_bitwise', 'fn main() { printi(1.5 & 2.0); }', b'require integers'),
        ('float_remainder', 'fn main() { printi(1.5 % 2.0); }', b'require integers'),
        ('float_shift', 'fn main() { printi(1.5 << 1); }', b'shift operators require integers'),
        ('float_printc', 'fn main() { printc(1.5); }', b'require an integer'),
        ('float_exit', 'fn main() { exit(1.5); }', b'require an integer'),
    ])
    cases.extend([
        ('int32_hex_patterns', 'fn main() { let h: i32 = 0x80000000; let f: i32 = 0xffffffff; let t: i32 = 0x7fffffff; printi(h); printi(f); printi(t); printi(f + 1); printi(h >> 31); }', b'-2147483648\n-1\n2147483647\n0\n-1\n', 0),
        ('int8_hex_patterns', 'fn main() { let h: i8 = 0x80; let f: i8 = 0xff; let t: i8 = 0x7f; printi(h); printi(f); printi(t); printi(f + 1); printi(h >> 7); }', b'-128\n-1\n127\n0\n-1\n', 0),
        ('int16_hex_patterns', 'fn main() { let h: i16 = 0x8000; let f: i16 = 0xffff; printi(h); printi(f); printi(f + 1); }', b'-32768\n-1\n0\n', 0),
        ('uint8_print', 'fn main() { let a: u8 = 255; let b: u8 = 0; printi(a); printi(b); printi(u8(-1)); printi(a - 1); }', b'255\n0\n255\n254\n', 0),
        ('int8_div_overflow', 'fn main() { let m: i8 = -128; printi(m / -1); }', b'', 71),
        ('int8_mod_overflow', 'fn main() { let m: i8 = -128; printi(m % -1); }', b'0\n', 0),
        ('int16_div_overflow', 'fn main() { let m: i16 = -32768; printi(m / -1); }', b'', 71),
        ('int8_aggregates', 'struct P { a:i8, b:u8 } fn mix(p:P, xs:[i8;2]) -> i16 { return i16(p.a) + i16(p.b) + i16(xs[0]) + i16(xs[1]); } fn main() { let p = P { a:-100, b:200 }; let xs: [i8;2] = [100, -50]; printi(mix(p, xs)); p.a = 127; printi(p.a); printi(xs[0]); }', b'150\n127\n100\n', 0),
        ('u32_print', 'fn main() { let a: u32 = 4294967295; let b: u32 = 0; printi(a); printi(b); printi(u32(-1)); printi(a - 1); }', b'4294967295\n0\n4294967295\n4294967294\n', 0),
        ('int32_div_zero', 'fn main() { let x: i32 = 5; printi(x / 0); }', b'', 70),
        ('int32_mod_zero', 'fn main() { let x: i32 = 5; printi(x % 0); }', b'', 70),
        ('int32_div_overflow', 'fn main() { let m: i32 = -2147483648; printi(m / -1); }', b'', 71),
        ('int32_mod_overflow', 'fn main() { let m: i32 = -2147483648; printi(m % -1); }', b'0\n', 0),
        ('u32_div_zero', 'fn main() { let x: u32 = 5; printi(x / 0); }', b'', 70),
        ('int32_casts', 'fn main() { let big = 0x123456789; printi(i32(big)); printi(u32(big)); printi(i64(u32(-1))); printi(u64(i32(-1))); printi(bool(5)); printi(bool(0)); }', b'591751049\n591751049\n4294967295\n18446744073709551615\n1\n0\n', 0),
        ('int32_aggregates', 'struct P { x:i32, y:u32 } fn sum(p:P, a:[i32;3]) -> i32 { return p.x + i32(p.y) + a[0] + a[1] + a[2]; } fn make(n:i32)->P { return P { x:n, y:u32(n) }; } fn main() { let p = make(-5); let a: [i32;3] = [1, 2, -3]; printi(sum(p, a)); a[0] = 100; printi(a[0]); printi(p.x); let q = p; q.x = 42; printi(p.x); printi(q.x); printi(len(a)); }', b'-10\n100\n-5\n-5\n42\n3\n', 0),
        ('int32_wrapping_loop', 'fn main() { let i: i32 = 0; let sum: i32 = 0; while(i < 100000) { sum = sum + i; i = i + 1; } printi(sum); }', b'704982704\n', 0),
        ('i64_default_no_two_gig', 'fn main() { let x = 3000000000; printi(x * 2); printi(x / 3); let y: u64 = 10000000000; printi(y); }', b'6000000000\n1000000000\n10000000000\n', 0),
    ])
    errors.extend([
        ('int32_literal_range', 'fn main() { let x: i32 = 2147483648; }', b'does not fit in i32'),
        ('u32_literal_range', 'fn main() { let x: u32 = 4294967296; }', b'does not fit in u32'),
        ('int8_literal_range', 'fn main() { let x: i8 = 128; }', b'does not fit in i8'),
        ('u8_literal_range', 'fn main() { let x: u8 = 256; }', b'does not fit in u8'),
        ('int16_literal_range', 'fn main() { let x: i16 = 32768; }', b'does not fit in i16'),
        ('u16_literal_range', 'fn main() { let x: u16 = 65536; }', b'does not fit in u16'),
        ('narrow_no_implicit', 'fn main() { let x: i32 = 0; let y: u32 = x; }', b'type mismatch'),
        ('narrow_param_range', 'fn f(x: i32) {} fn main() { f(5000000000); }', b'does not fit in i32'),
    ])
    with tempfile.TemporaryDirectory(prefix='aegis-tests-') as folder:
        folder = Path(folder)
        notices=run(COMPILER,'--licenses')
        assert notices.returncode==0 and b'iced project and contributors' in notices.stdout and b'lazy_static' in notices.stdout
        print('PASS embedded third-party notices')
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
                     'return_32_arguments', 'array_negative_read', 'division_zero',
                     'dyn_heap_stress', 'dyn_float_elements', 'dyn_unique_set_stress',
                     'dyn_grow_string_unique', 'dyn_grow_nested_field']:
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
        listing=run(COMPILER,'disasm',inspected)
        assert listing.returncode==0 and b'Linear disassembly' in listing.stdout and b'file 00000400' in listing.stdout,(listing.stdout,listing.stderr)
        assert inspected.read_bytes()==before
        print('PASS read-only disassembly of generated code')
        for arguments,expected in [(('0x1000','0'),b'limit exceeded'),(('0x100000000',),b'RVA must')]:
            result=run(COMPILER,'disasm',inspected,*arguments)
            assert result.returncode==2 and expected in result.stderr and not result.stdout
            print('PASS reject disassembly arguments',arguments)
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
            listing=run(COMPILER,'disasm',exported,'0x2000','1')
            assert listing.returncode==0 and b'ret' in listing.stdout and b'file 00000C00' in listing.stdout,(listing.stdout,listing.stderr)
            assert exported.read_bytes()==fixture
            print('PASS fixture disassembly',bits)
            if bits==64:
                unsupported=bytearray(fixture);unsupported[0x84:0x86]=(0xaa64).to_bytes(2,'little')
                exported.write_bytes(unsupported)
                result=run(COMPILER,'disasm',exported,'0x2000','1')
                assert result.returncode==2 and b'PE machine must' in result.stderr and not result.stdout
                print('PASS non-x86 PE disassembly rejected')
                truncated=bytearray(fixture);truncated[0xc00]=0x0f
                truncated[0x188+40+8:0x188+40+12]=(1).to_bytes(4,'little')
                truncated[0x188+40+16:0x188+40+20]=(1).to_bytes(4,'little')
                exported.write_bytes(truncated)
                result=run(COMPILER,'disasm',exported,'0x2000','1')
                assert result.returncode==0 and b'[invalid]' in result.stdout,(result.stdout,result.stderr)
                print('PASS truncated instruction visibly marked')
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
    print(f'{len(cases) + len(errors) + 43} checks passed')


if __name__ == '__main__':
    main()
