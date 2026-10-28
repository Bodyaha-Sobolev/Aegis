//! Small, deterministic x86-64 backend. Every write stays inside an owned Vec.
use crate::{
    Diagnostic,
    lexer::Keyword,
    parser::{Expr, ExprKind, Program, Stmt, StmtKind, Type, is_place},
};
use std::{collections::HashMap, ops::Range};

const TEXT: u32 = 0x1000;
const IMPORTS: u32 = 0x11000;
const MAX_CODE: usize = 0x10000;
const RDATA: u32 = 0x12000;
const MAX_DATA: usize = 2 * 1024 * 1024;
// Tagged dynamic-array pointer: rc at -1, len at +7, capacity at +15.
const DYN_DATA: u8 = 23;
const MAX_DYN_ELEMENTS: u32 = 1_048_576;
// Offsets of the f64 constant pool inside .rdata.
const DATA_HALF: usize = 8;
const DATA_MILLION: usize = 16;
const DATA_TWO_POW_63: usize = 24;
const DATA_NAN: usize = 32;
const DATA_INF: usize = 40;
const DATA_NEG_INF: usize = 48;
const DATA_SIGN: usize = 56;
const DATA_SIGN_32: usize = 64;
const DATA_TWO_POW_63_32: usize = 68;

pub(crate) fn generate(source: &str, program: &Program) -> Result<(Vec<u8>, usize), Diagnostic> {
    let mut g = Generator {
        source,
        code: Vec::new(),
        functions: program
            .functions
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.clone(), (i, f.params.len())))
            .collect(),
        offsets: vec![0; program.functions.len()],
        calls: Vec::new(),
        vars: Vec::new(),
        slots: 0,
        peak_slots: 0,
        aggregate_result: false,
        stack: 0,
        loops: Vec::new(),
        printi: 0,
        printu: 0,
        printc: 0,
        printstr: 0,
        streq: 0,
        data: {
            // Static empty-string descriptor, then the f64 constant pool and
            // printd spellings (offsets 8/16/24/32/40/48/56).
            let mut data = vec![0; 8];
            data.extend_from_slice(&0.5f64.to_bits().to_le_bytes());
            data.extend_from_slice(&1_000_000.0f64.to_bits().to_le_bytes());
            data.extend_from_slice(&9223372036854775808.0f64.to_bits().to_le_bytes());
            data.extend_from_slice(b"nan\n\0\0\0\0");
            data.extend_from_slice(b"inf\n\0\0\0\0");
            data.extend_from_slice(b"-inf\n\0\0\0");
            data.extend_from_slice(&0x8000000000000000u64.to_le_bytes());
            data.extend_from_slice(&0x0000000080000000u32.to_le_bytes());
            data.extend_from_slice(&0x5f000000u32.to_le_bytes());
            data
        },
        printd: 0,
        rt_alloc: 0,
        rt_retain: 0,
        rt_release: 0,
        rt_concat: 0,
        rt_strlen: 0,
        rt_sbyte: 0,
        rt_dnew: 0,
        rt_dlen: 0,
        rt_didx: 0,
        rt_dset_str: 0,
        rt_dset_plain: 0,
        rt_push_str: 0,
        rt_push_plain: 0,
        rt_drelease_str: 0,
        rt_dfree_str: 0,
        literals: HashMap::from([(String::new(), 0)]),
    };
    let main_patch = g.runtime();
    for (index, function) in program.functions.iter().enumerate() {
        g.offsets[index] = g.code.len();
        g.vars.clear();
        g.aggregate_result = function.result.aggregate();
        g.slots = usize::from(g.aggregate_result);
        g.peak_slots = g.slots;
        g.stack = 0;
        g.bytes(&[0x55, 0x48, 0x89, 0xe5, 0x48, 0x81, 0xec]);
        let frame = g.hole();
        if g.aggregate_result {
            // Hidden caller-owned result buffer; keep it separate from the
            // 256 source-visible slots and preserve it across nested calls.
            g.bytes(&[0x4c, 0x89, 0x95]);
            g.dword((-8i32) as u32);
        }
        // Each of the first four positions selects either its GP register
        // (RCX/RDX/R8/R9) or XMM0/1/2/3 according to the parameter type.
        for (i, param) in function.params.iter().enumerate() {
            let slot = g.declare(&param.name, param.ty.clone(), &function.span)?;
            if i >= 4 {
                // RBP points to the saved frame pointer, followed by return
                // address, four shadow slots, then the fifth argument.
                g.bytes(&[0x48, 0x8b, 0x85]);
                g.dword((48 + (i - 4) * 8) as u32);
                g.local(0x89, slot);
                continue;
            }
            if param.ty.float() {
                g.bytes(&[0xf2, 0x0f, 0x11, 0x85 | ((i as u8) << 3)]);
            } else {
                g.bytes(
                    &[
                        [0x48, 0x89, 0x8d],
                        [0x48, 0x89, 0x95],
                        [0x4c, 0x89, 0x85],
                        [0x4c, 0x89, 0x8d],
                    ][i],
                );
            }
            g.dword((-(8 * (slot as i32 + 1))) as u32);
        }
        // All incoming registers must be saved before any aggregate copy uses RCX.
        // Aggregate parameters receive caller snapshots, then own independent slots.
        for param in &function.params {
            if param.ty.aggregate() {
                let slot = g.variable(&param.name, &function.span)?;
                g.local(0x8b, slot);
                g.bytes(&[0x48, 0x89, 0xc1]);
                for element in 0..param.ty.slots() {
                    g.bytes(&[0x48, 0x8b, 0x81]);
                    g.dword((element * 8) as u32);
                    g.local(0x89, slot + param.ty.slots() - 1 - element);
                }
                for (string, _) in
                    Generator::str_slot_list(&param.ty.clone(), slot + param.ty.slots() - 1)
                {
                    g.local(0x8b, string);
                    g.emit_retain();
                }
            }
        }
        // External callers may leave upper GP/stack-slot bits unspecified for
        // narrow arguments. All incoming registers have already been saved.
        for param in &function.params {
            if param.ty.narrow() || param.ty == Type::Bool {
                let slot = g.variable(&param.name, &function.span)?;
                g.local(0x8b, slot);
                if param.ty == Type::Bool {
                    g.bytes(&[0x84, 0xc0, 0x0f, 0x95, 0xc0, 0x48, 0x0f, 0xb6, 0xc0]);
                } else {
                    g.canonical(&param.ty);
                }
                g.local(0x89, slot);
            }
        }
        g.block(&function.body)?;
        g.release_scope(0);
        // Win64 calls require 16-byte stack alignment. Slots from disjoint
        // lexical scopes share space; reserve only the maximum live count.
        let frame_size = (g.peak_slots * 8).next_multiple_of(16) as u32;
        g.code[frame..frame + 4].copy_from_slice(&frame_size.to_le_bytes());
        if g.aggregate_result {
            g.bytes(&[0x0f, 0x0b]); // Definite-return checking makes this unreachable.
        } else if function.result == Type::Str {
            g.string_literal("")?;
        } else if function.result.dyn_array() {
            // As with str, falling through returns an empty owned value.
            g.bytes(&[0x31, 0xc9]);
            let amount = g.reserve_call();
            g.bytes(&[0xe8]);
            g.relative(g.rt_dnew);
            g.release_call(amount);
        } else if function.result.float() {
            g.bytes(&[0x66, 0x0f, 0xef, 0xc0]); // pxor xmm0, xmm0
        } else {
            g.bytes(&[0x31, 0xc0]);
        }
        g.bytes(&[0xc9, 0xc3]);
    }
    let Some(&(main, _)) = g.functions.get("main") else {
        return Err(g.error(&(0..0), "fn main() is required"));
    };
    g.patch(main_patch, g.offsets[main]);
    for &(patch, index) in &g.calls {
        let target = g.offsets[index];
        patch_relative(&mut g.code, patch, target);
    }
    if g.code.len() > MAX_CODE {
        return Err(g.error(&(0..0), "native code limit exceeded (64 KiB)"));
    }
    let size = g.code.len();
    Ok((emit_pe(&g.code, &g.data), size))
}

struct Loop {
    start: usize,
    breaks: Vec<usize>,
    vars: usize,
}
struct Generator<'a> {
    source: &'a str,
    code: Vec<u8>,
    functions: HashMap<String, (usize, usize)>,
    offsets: Vec<usize>,
    calls: Vec<(usize, usize)>,
    vars: Vec<(String, usize, Type)>,
    slots: usize,
    peak_slots: usize,
    aggregate_result: bool,
    stack: usize,
    loops: Vec<Loop>,
    printi: usize,
    printu: usize,
    printc: usize,
    printstr: usize,
    streq: usize,
    printd: usize,
    rt_alloc: usize,
    rt_retain: usize,
    rt_release: usize,
    rt_concat: usize,
    rt_strlen: usize,
    rt_sbyte: usize,
    rt_dnew: usize,
    rt_dlen: usize,
    rt_didx: usize,
    rt_dset_str: usize,
    rt_dset_plain: usize,
    rt_push_str: usize,
    rt_push_plain: usize,
    rt_drelease_str: usize,
    rt_dfree_str: usize,
    data: Vec<u8>,
    literals: HashMap<String, usize>,
}
impl Generator<'_> {
    fn string_literal(&mut self, value: &str) -> Result<(), Diagnostic> {
        let offset = if let Some(offset) = self.literals.get(value) {
            *offset
        } else {
            let offset = self.data.len().next_multiple_of(8);
            if offset + 8 + value.len() > MAX_DATA {
                return Err(self.error(&(0..0), "string data limit exceeded (2 MiB)"));
            }
            self.data.resize(offset, 0);
            self.data
                .extend_from_slice(&(value.len() as u64).to_le_bytes());
            self.data.extend_from_slice(value.as_bytes());
            self.literals.insert(value.to_owned(), offset);
            offset
        };
        self.bytes(&[0x48, 0x8d, 0x05]);
        self.dword(RDATA + offset as u32 - (TEXT + self.code.len() as u32 + 4));
        Ok(())
    }
    fn error(&self, span: &Range<usize>, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(self.source, span.clone(), message)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }
    fn dword(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }
    fn hole(&mut self) -> usize {
        let at = self.code.len();
        self.dword(0);
        at
    }
    fn jump(&mut self, condition: Option<u8>) -> usize {
        if let Some(c) = condition {
            self.bytes(&[0x0f, c]);
        } else {
            self.bytes(&[0xe9]);
        }
        self.hole()
    }
    fn patch(&mut self, at: usize, target: usize) {
        patch_relative(&mut self.code, at, target);
    }
    fn here(&mut self, at: usize) {
        self.patch(at, self.code.len());
    }
    fn relative(&mut self, target: usize) {
        let at = self.hole();
        self.patch(at, target);
    }
    fn push(&mut self) {
        self.bytes(&[0x50]);
        self.stack += 1;
    }
    fn pop(&mut self, bytes: &[u8]) {
        self.bytes(bytes);
        self.stack -= 1;
    }
    // Push consumes an owned array and element. Unique arrays reuse capacity;
    // growth moves their elements, while shared arrays copy/retain them.
    fn dynamic_push(&mut self, strings: bool) -> usize {
        let entry = self.code.len();
        // rbx=array, r12=element, r13=fresh block, r14=index, rsi=shared flag.
        // Five saved registers align rsp; the 32-byte callee area stays separate.
        self.bytes(&[0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x56]);
        self.bytes(&[0x48, 0x83, 0xec, 0x20, 0x48, 0x89, 0xcb, 0x49, 0x89, 0xd4]);
        self.bytes(&[0x48, 0x83, 0x7b, 0xff, 1]); // rc == 1
        let shared = self.jump(Some(0x85));
        self.bytes(&[0x48, 0x8b, 0x43, 7, 0x48, 0x3b, 0x43, 15]); // len < capacity
        let grow = self.jump(Some(0x83));
        self.bytes(&[0x4c, 0x89, 0x64, 0xc3, DYN_DATA]);
        self.bytes(&[0x48, 0xff, 0x43, 7, 0x48, 0x89, 0xd8]);
        let reused = self.jump(None);
        self.here(grow);
        self.bytes(&[0x31, 0xf6]); // unique, move elements on growth
        let allocate = self.jump(None);
        self.here(shared);
        self.bytes(&[0xbe, 1, 0, 0, 0]); // shared, retain copied elements
        self.here(allocate);
        self.bytes(&[0x48, 0x8b, 0x4b, 7, 0x48, 0xff, 0xc1, 0xe8]);
        self.relative(self.rt_dnew);
        self.bytes(&[0x49, 0x89, 0xc5, 0x45, 0x31, 0xf6]); // r13=fresh, r14=index
        let copy = self.code.len();
        self.bytes(&[0x4c, 0x3b, 0x73, 7]);
        let copied = self.jump(Some(0x83));
        self.bytes(&[0x4a, 0x8b, 0x4c, 0xf3, DYN_DATA]);
        if strings {
            self.bytes(&[0x48, 0x85, 0xf6]);
            let moved = self.jump(Some(0x84));
            self.bytes(&[0xe8]);
            self.relative(self.rt_retain);
            self.bytes(&[0x4a, 0x8b, 0x4c, 0xf3, DYN_DATA]);
            self.here(moved);
        }
        self.bytes(&[0x4b, 0x89, 0x4c, 0xf5, DYN_DATA, 0x49, 0xff, 0xc6]);
        let again = self.jump(None);
        self.patch(again, copy);
        self.here(copied);
        self.bytes(&[0x48, 0x8b, 0x43, 7, 0x4d, 0x89, 0x64, 0xc5, DYN_DATA]);
        self.bytes(&[0x48, 0x89, 0xd9]);
        if strings {
            self.bytes(&[0x48, 0x85, 0xf6]);
            let moved = self.jump(Some(0x84));
            self.bytes(&[0xe8]);
            self.relative(self.rt_drelease_str);
            let released = self.jump(None);
            self.here(moved);
            // Only the old allocation is released after moving its references.
            self.bytes(&[0xe8]);
            self.relative(self.rt_release);
            self.here(released);
        } else {
            self.bytes(&[0xe8]);
            self.relative(self.rt_release);
        }
        self.bytes(&[0x4c, 0x89, 0xe8]);
        self.here(reused);
        self.bytes(&[0x48, 0x83, 0xc4, 0x20]);
        self.bytes(&[0x5e, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0xc3]);
        entry
    }
    // Element assignment consumes an owned reference; the destination cell
    // remains an owner until the caller stores the result.
    fn dynamic_set(&mut self, strings: bool) -> usize {
        let entry = self.code.len();
        self.bytes(&[0x53, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x56]);
        self.bytes(&[0x48, 0x83, 0xec, 0x20, 0x48, 0x89, 0xcb]);
        self.bytes(&[0x49, 0x89, 0xd4, 0x4d, 0x89, 0xc5]); // index, element
        self.bytes(&[0x48, 0x3b, 0x53, 7]);
        let in_bounds = self.jump(Some(0x82));
        self.bytes(&[0xb9]);
        self.dword(72);
        self.bytes(&[0x41, 0xff, 0x57, 0x10]);
        self.here(in_bounds);
        // Exactly two references: destination cell plus the consumed argument.
        self.bytes(&[0x48, 0x83, 0x7b, 0xff, 2]);
        let shared = self.jump(Some(0x85));
        if strings {
            self.bytes(&[0x4a, 0x8b, 0x4c, 0xe3, DYN_DATA, 0xe8]);
            self.relative(self.rt_release);
        }
        self.bytes(&[0x4e, 0x89, 0x6c, 0xe3, DYN_DATA, 0x48, 0x89, 0xd8]);
        let reused = self.jump(None);
        self.here(shared);
        self.bytes(&[0x48, 0x8b, 0x4b, 7, 0xe8]);
        self.relative(self.rt_dnew);
        self.bytes(&[0x49, 0x89, 0xc6, 0x31, 0xf6]); // r14=fresh, rsi=index
        let copy = self.code.len();
        self.bytes(&[0x48, 0x3b, 0x73, 7]);
        let copied = self.jump(Some(0x83));
        self.bytes(&[0x4c, 0x39, 0xe6]);
        let skip = self.jump(Some(0x84)); // replaced element needs no new reference
        self.bytes(&[0x48, 0x8b, 0x4c, 0xf3, DYN_DATA]);
        if strings {
            self.bytes(&[0xe8]);
            self.relative(self.rt_retain);
            self.bytes(&[0x48, 0x8b, 0x4c, 0xf3, DYN_DATA]);
        }
        self.bytes(&[0x49, 0x89, 0x4c, 0xf6, DYN_DATA]);
        self.here(skip);
        self.bytes(&[0x48, 0xff, 0xc6]);
        let again = self.jump(None);
        self.patch(again, copy);
        self.here(copied);
        self.bytes(&[0x4f, 0x89, 0x6c, 0xe6, DYN_DATA, 0x48, 0x89, 0xd9, 0xe8]);
        self.relative(if strings {
            self.rt_drelease_str
        } else {
            self.rt_release
        });
        self.bytes(&[0x4c, 0x89, 0xf0]);
        self.here(reused);
        self.bytes(&[0x48, 0x83, 0xc4, 0x20]);
        self.bytes(&[0x5e, 0x41, 0x5e, 0x41, 0x5d, 0x41, 0x5c, 0x5b, 0xc3]);
        entry
    }
    fn reserve_call(&mut self) -> u8 {
        let amount = if self.stack.is_multiple_of(2) { 32 } else { 40 };
        self.bytes(&[0x48, 0x83, 0xec, amount]);
        amount
    }
    fn release_call(&mut self, amount: u8) {
        self.bytes(&[0x48, 0x83, 0xc4, amount]);
    }
    fn exit(&mut self, code: u32) {
        self.bytes(&[0xb9]);
        self.dword(code);
        self.reserve_call();
        self.bytes(&[0x41, 0xff, 0x57, 0x10]);
    }
    fn declare(&mut self, name: &str, ty: Type, span: &Range<usize>) -> Result<usize, Diagnostic> {
        if self.vars.iter().any(|(n, _, _)| n == name) {
            return Err(self.error(span, format!("variable already defined: {name}")));
        }
        if self.slots + ty.slots() > 256 + usize::from(self.aggregate_result) {
            return Err(self.error(span, "local variable limit exceeded"));
        }
        let slot = self.slots;
        self.slots += ty.slots();
        self.peak_slots = self.peak_slots.max(self.slots);
        self.vars.push((name.to_owned(), slot, ty));
        Ok(slot)
    }
    fn variable(&self, name: &str, span: &Range<usize>) -> Result<usize, Diagnostic> {
        self.vars
            .iter()
            .rev()
            .find(|(n, _, _)| n == name)
            .map(|(_, s, _)| *s)
            .ok_or_else(|| self.error(span, format!("undefined variable: {name}")))
    }
    // Offsets (relative to a base) that hold owned values, in
    // snapshot-address order: field 0 sits at the lowest address. The flag
    // marks dynamic arrays of str (their release walks the elements).
    fn str_offsets(ty: &Type, base: usize, out: &mut Vec<(usize, bool)>) {
        if *ty == Type::Str {
            out.push((base, false));
        } else if let Type::Dyn(element) = ty {
            out.push((base, **element == Type::Str));
        } else if let Type::Record(record) = ty {
            let mut offset = base;
            for (_, field) in &record.fields {
                Self::str_offsets(field, offset, out);
                offset += field.slots();
            }
        } else if let Some(element) = ty.element() {
            let stride = element.slots();
            for index in 0..ty.len().expect("checked array") {
                Self::str_offsets(&element, base + index * stride, out);
            }
        }
    }
    // The same walk over rbp-relative slot indices: addresses descend as the
    // slot index grows, so records store field 0 at the highest slot.
    fn str_slots(ty: &Type, top: usize, out: &mut Vec<(usize, bool)>) {
        if *ty == Type::Str {
            out.push((top, false));
        } else if let Type::Dyn(element) = ty {
            let str_elements = **element == Type::Str;
            out.push((top, str_elements));
        } else if let Type::Record(record) = ty {
            let mut cursor = top;
            for (index, (_, field)) in record.fields.iter().enumerate() {
                let field_slots = field.slots();
                Self::str_slots(field, cursor, out);
                if index + 1 < record.fields.len() {
                    cursor -= field_slots;
                }
            }
        } else if let Some(element) = ty.element() {
            let stride = element.slots();
            for index in 0..ty.len().expect("checked array") {
                Self::str_slots(&element, top - index * stride, out);
            }
        }
    }
    fn str_slot_list(ty: &Type, top: usize) -> Vec<(usize, bool)> {
        let mut slots = Vec::new();
        Self::str_slots(ty, top, &mut slots);
        slots
    }
    // rcx already holds the value.
    fn emit_retain(&mut self) {
        self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax
        self.emit_retain_rcx();
    }
    fn emit_retain_rcx(&mut self) {
        let amount = self.reserve_call();
        self.bytes(&[0xe8]);
        self.relative(self.rt_retain);
        self.release_call(amount);
    }
    fn emit_release_rcx(&mut self) {
        self.emit_release(false);
    }
    fn emit_release(&mut self, strings: bool) {
        let amount = self.reserve_call();
        self.bytes(&[0xe8]);
        self.relative(if strings {
            self.rt_drelease_str
        } else {
            self.rt_release
        });
        self.release_call(amount);
    }
    fn release_slot(&mut self, slot: usize, strings: bool) {
        self.local(0x8b, slot);
        self.bytes(&[0x48, 0x89, 0xc1]);
        self.emit_release(strings);
    }
    fn string_elements(ty: &Type) -> bool {
        matches!(ty, Type::Dyn(element) if **element == Type::Str)
    }
    fn scalar_bits(&mut self, ty: &Type) {
        if ty.single() {
            self.bytes(&[0x66, 0x0f, 0x7e, 0xc0]); // movd eax, xmm0
        } else if ty.float() {
            self.bytes(&[0x66, 0x48, 0x0f, 0x7e, 0xc0]); // movq rax, xmm0
        }
    }
    fn release_scope(&mut self, from: usize) {
        let mut slots = Vec::new();
        for (_, slot, ty) in &self.vars[from..] {
            slots.extend(Self::str_slot_list(ty, slot + ty.slots() - 1));
        }
        for (slot, str_elements) in slots {
            self.release_slot(slot, str_elements);
        }
    }
    // Evaluate a string expression as an owned temporary: places are
    // retained, computed values already carry their own reference.
    fn str_temp(&mut self, expr: &Expr) -> Result<(), Diagnostic> {
        self.expr(expr)?;
        if expr.ty.owned() && is_place(expr) {
            self.emit_retain();
        }
        Ok(())
    }
    fn local(&mut self, opcode: u8, slot: usize) {
        self.bytes(&[0x48, opcode, 0x85]);
        self.dword((-(8 * (slot as i32 + 1))) as u32);
    }
    // Narrow results re-enter canonical form: signed widths sign-extended,
    // unsigned zero-extended. Redundant on already-canonical values.
    fn canonical(&mut self, ty: &Type) {
        match ty {
            Type::I32 => self.bytes(&[0x48, 0x63, 0xc0]), // movsxd rax, eax
            Type::U32 => self.bytes(&[0x89, 0xc0]),       // mov eax, eax
            Type::I16 => self.bytes(&[0x48, 0x0f, 0xbf, 0xc0]), // movsx rax, ax
            Type::U16 => self.bytes(&[0x48, 0x0f, 0xb7, 0xc0]), // movzx rax, ax
            Type::I8 => self.bytes(&[0x48, 0x0f, 0xbe, 0xc0]), // movsx rax, al
            Type::U8 => self.bytes(&[0x48, 0x0f, 0xb6, 0xc0]), // movzx rax, al
            _ => {}
        }
    }
    // Narrow shifts operate within the width and the count is taken modulo
    // that width, so CL is masked for 8/16-bit operands (hardware masks to 31).
    fn narrow_shift(&mut self, ty: &Type, op: u8) {
        match ty.bits() {
            8 => {
                self.bytes(&[0x80, 0xe1, 0x07]); // and cl, 7
                self.bytes(&[0xd2, op]); // shift al, cl
            }
            16 => {
                self.bytes(&[0x80, 0xe1, 0x0f]); // and cl, 15
                self.bytes(&[0x66, 0xd3, op]); // shift ax, cl
            }
            _ => self.bytes(&[0xd3, op]), // shift eax, cl
        }
        self.canonical(ty);
    }
    // RIP-relative reference into the .rdata constant pool. The displacement
    // runs from the end of the rel32, in the same TEXT-based frame that
    // string_literal uses for its own references.
    fn data_ref(&mut self, opcode: &[u8], offset: usize) {
        self.bytes(opcode);
        let at = self.hole();
        // at is the position of the rel32 itself; rip after it is TEXT+at+4.
        let disp = RDATA + offset as u32 - (TEXT + at as u32 + 4);
        self.code[at..at + 4].copy_from_slice(&disp.to_le_bytes());
    }
    fn fpush(&mut self) {
        self.bytes(&[0x48, 0x83, 0xec, 0x08]); // sub rsp, 8
        self.bytes(&[0xf2, 0x0f, 0x11, 0x04, 0x24]); // movsd [rsp], xmm0
        self.stack += 1;
    }
    fn fpop_load(&mut self) {
        self.bytes(&[0xf2, 0x0f, 0x10, 0x04, 0x24]); // movsd xmm0, [rsp]
        self.bytes(&[0x48, 0x83, 0xc4, 0x08]); // add rsp, 8
        self.stack -= 1;
    }
    fn load_f64(&mut self, slot: usize) {
        self.bytes(&[0xf2, 0x0f, 0x10, 0x85]); // movsd xmm0, [rbp-disp]
        self.dword((-(8 * (slot as i32 + 1))) as u32);
    }
    fn store_f64(&mut self, slot: usize) {
        self.bytes(&[0xf2, 0x0f, 0x11, 0x85]); // movsd [rbp-disp], xmm0
        self.dword((-(8 * (slot as i32 + 1))) as u32);
    }
    fn load_f32(&mut self, slot: usize) {
        self.bytes(&[0xf3, 0x0f, 0x10, 0x85]); // movss xmm0, [rbp-disp]
        self.dword((-(8 * (slot as i32 + 1))) as u32);
    }
    fn store_f32(&mut self, slot: usize) {
        self.bytes(&[0xf3, 0x0f, 0x11, 0x85]); // movss [rbp-disp], xmm0
        self.dword((-(8 * (slot as i32 + 1))) as u32);
    }
    // Conditions accept f64: true iff not NaN and not zero.
    fn condition_from_float(&mut self, ty: &Type) {
        if !ty.float() {
            return;
        }
        self.bytes(&[0x66, 0x0f, 0xef, 0xc9]); // pxor xmm1, xmm1
        if ty.single() {
            self.bytes(&[0x0f, 0x2e, 0xc1]); // ucomiss xmm0, xmm1
        } else {
            self.bytes(&[0x66, 0x0f, 0x2e, 0xc1]); // ucomisd xmm0, xmm1
        }
        self.bytes(&[0x0f, 0x95, 0xc0]); // setne al
        self.bytes(&[0x0f, 0x9b, 0xc1]); // setnp cl
        self.bytes(&[0x22, 0xc1]); // and al, cl
        self.bytes(&[0x48, 0x0f, 0xb6, 0xc0]); // movzx rax, al
    }
    fn block(&mut self, body: &[Stmt]) -> Result<(), Diagnostic> {
        let scope = self.vars.len();
        let slots = self.slots;
        for stmt in body {
            self.statement(stmt)?;
            if self.code.len() > MAX_CODE {
                return Err(self.error(&stmt.span, "native code limit exceeded (64 KiB)"));
            }
        }
        self.release_scope(scope);
        self.vars.truncate(scope);
        self.slots = slots;
        Ok(())
    }
    fn restore_stack(&mut self, stack: usize) {
        if self.stack > stack {
            self.bytes(&[0x48, 0x81, 0xc4]);
            self.dword(((self.stack - stack) * 8) as u32);
            self.stack = stack;
        }
    }
    fn snapshot_store(&mut self, end: usize, offset: usize) {
        self.bytes(&[0x48, 0x89, 0x84, 0x24]);
        self.dword(((self.stack - end + offset) * 8) as u32);
    }
    fn write_snapshot(
        &mut self,
        value: &Expr,
        end: usize,
        offset: usize,
    ) -> Result<(), Diagnostic> {
        if value.ty.dyn_array() {
            self.str_temp(value)?;
            self.snapshot_store(end, offset);
            return Ok(());
        }
        match &value.kind {
            ExprKind::Record(record, fields) => {
                for (name, value) in fields {
                    let field_offset: usize = record
                        .fields
                        .iter()
                        .take_while(|(field, _)| field != name)
                        .map(|(_, ty)| ty.slots())
                        .sum();
                    self.write_snapshot(value, end, offset + field_offset)?;
                }
            }
            ExprKind::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    self.write_snapshot(value, end, offset + index * value.ty.slots())?;
                }
            }
            ExprKind::Repeat(value, count) => {
                // Compute the element once, including aggregate constructors and
                // their side effects, then copy the completed element snapshot.
                self.write_snapshot(value, end, offset)?;
                let mut owned = Vec::new();
                Self::str_offsets(&value.ty.clone(), 0, &mut owned);
                for index in 1..*count {
                    for slot in 0..value.ty.slots() {
                        self.bytes(&[0x48, 0x8b, 0x84, 0x24]);
                        self.dword(((self.stack - end + offset + slot) * 8) as u32);
                        self.snapshot_store(end, offset + index * value.ty.slots() + slot);
                    }
                    // Each extra copy shares the element's pointers until it
                    // takes its own reference.
                    for (string, _) in &owned {
                        self.bytes(&[0x48, 0x8b, 0x8c, 0x24]);
                        self.dword(
                            ((self.stack - end + offset + index * value.ty.slots() + string) * 8)
                                as u32,
                        ); // mov rcx, [rsp+..]
                        self.emit_retain_rcx();
                    }
                }
            }
            _ => {
                let initial_stack = self.stack;
                self.expr(value)?;
                if value.ty.owned() && is_place(value) {
                    self.emit_retain();
                }
                if value.ty.aggregate() {
                    self.bytes(&[0x48, 0x89, 0xc1]);
                    for index in 0..value.ty.slots() {
                        self.bytes(&[0x48, 0x8b, 0x81]);
                        self.dword((index * 8) as u32);
                        self.snapshot_store(end, offset + index);
                    }
                    if is_place(value) {
                        // A place keeps its own references; the snapshot slots
                        // take copies, so each owned value gains one here.
                        let mut owned = Vec::new();
                        Self::str_offsets(&value.ty.clone(), offset, &mut owned);
                        for (offset, _) in owned {
                            self.bytes(&[0x48, 0x8b, 0x8c, 0x24]);
                            self.dword(((self.stack - end + offset) * 8) as u32); // mov rcx, [rsp+..]
                            self.emit_retain_rcx();
                        }
                    }
                } else if value.ty.single() {
                    self.bytes(&[0xf3, 0x0f, 0x11, 0x84, 0x24]); // movss [rsp+..], xmm0
                    self.dword(((self.stack - end + offset) * 8) as u32);
                } else if value.ty.float() {
                    self.bytes(&[0xf2, 0x0f, 0x11, 0x84, 0x24]); // movsd [rsp+..], xmm0
                    self.dword(((self.stack - end + offset) * 8) as u32);
                } else {
                    self.snapshot_store(end, offset);
                }
                self.restore_stack(initial_stack);
            }
        }
        Ok(())
    }
    fn snapshot(&mut self, value: &Expr) -> Result<(), Diagnostic> {
        // Touch each slot while growing the stack. All RHS evaluations finish
        // before the caller copies the snapshot to a possibly overlapping place.
        self.bytes(&[0x31, 0xc0]);
        for _ in 0..value.ty.slots() {
            self.push();
        }
        self.write_snapshot(value, self.stack, 0)
    }
    fn store_aggregate(
        &mut self,
        slot: usize,
        value: &Expr,
        assign: bool,
    ) -> Result<(), Diagnostic> {
        self.snapshot(value)?;
        if assign {
            for (offset, str_elements) in
                Self::str_slot_list(&value.ty.clone(), slot + value.ty.slots() - 1)
            {
                self.release_slot(offset, str_elements);
            }
        }
        for index in 0..value.ty.slots() {
            self.pop(&[0x58]);
            self.local(0x89, slot + value.ty.slots() - 1 - index);
        }
        Ok(())
    }
    fn place_address(&mut self, place: &Expr) -> Result<(), Diagnostic> {
        if let ExprKind::Var(name) = &place.kind {
            let slot = self.variable(name, &place.span)?;
            self.local(0x8d, slot);
            Ok(())
        } else {
            self.index_address(place)
        }
    }
    fn index_address(&mut self, target: &Expr) -> Result<(), Diagnostic> {
        if let ExprKind::Field(base, name) = &target.kind {
            let Type::Record(record) = &base.ty else {
                return Err(self.error(&target.span, "field access requires a struct"));
            };
            let offset: usize = record
                .fields
                .iter()
                .take_while(|(field, _)| field != name)
                .map(|(_, ty)| ty.slots())
                .sum();
            self.expr(base)?;
            self.bytes(&[0x48, 0x05]);
            self.dword((offset * 8) as u32);
            return Ok(());
        }
        let ExprKind::Index(array, index) = &target.kind else {
            return Err(self.error(&target.span, "expected array element"));
        };
        if array.ty == Type::Str {
            self.str_temp(array)?;
            self.push(); // [rsp] = the owned string
            self.expr(index)?;
            self.bytes(&[0x48, 0x89, 0xc2]); // mov rdx, rax
            self.bytes(&[0x48, 0x8b, 0x0c, 0x24]); // mov rcx, [rsp]
            let amount = self.reserve_call();
            self.bytes(&[0xe8]);
            self.relative(self.rt_sbyte);
            self.release_call(amount);
            self.push();
            self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 8]);
            self.emit_release_rcx();
            self.pop(&[0x58]);
            self.pop(&[0x59]);
            return Ok(());
        }
        if array.ty.dyn_array() {
            self.str_temp(array)?;
            self.push();
            self.expr(index)?;
            self.bytes(&[0x48, 0x89, 0xc2, 0x48, 0x8b, 0x0c, 0x24]);
            let amount = self.reserve_call();
            self.bytes(&[0xe8]);
            self.relative(self.rt_didx);
            self.release_call(amount);
            if target.ty.owned() {
                self.emit_retain();
            }
            self.push(); // element survives releasing the array
            self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 8]);
            self.emit_release(Self::string_elements(&array.ty));
            self.pop(&[0x58]);
            self.pop(&[0x59]);
            if target.ty.float() {
                self.bytes(&[0x66, 0x48, 0x0f, 0x6e, 0xc0]);
            }
            return Ok(());
        }
        let len = array.ty.len().expect("checked array type");
        let stride = array.ty.element().expect("checked element").slots();
        self.expr(array)?;
        self.push();
        self.expr(index)?;
        self.bytes(&[0x48, 0x3d]);
        self.dword(len as u32);
        let in_bounds = self.jump(Some(0x82));
        self.exit(72);
        self.here(in_bounds);
        self.pop(&[0x59]);
        if stride == 1 {
            self.bytes(&[0x48, 0x8d, 0x04, 0xc1]);
        } else {
            let bytes = stride * 8;
            if bytes.is_power_of_two() {
                self.bytes(&[0x48, 0xc1, 0xe0, bytes.trailing_zeros() as u8]);
            } else {
                self.bytes(&[0x48, 0x69, 0xc0]);
                self.dword(bytes as u32);
            }
            self.bytes(&[0x48, 0x01, 0xc8]);
        }
        Ok(())
    }
    fn same_storage_path(left: &Expr, right: &Expr) -> bool {
        match (&left.kind, &right.kind) {
            (ExprKind::Var(a), ExprKind::Var(b)) => a == b,
            (ExprKind::Field(a, x), ExprKind::Field(b, y)) => {
                x == y && Self::same_storage_path(a, b)
            }
            // Index expressions may have side effects; retain the general path.
            _ => false,
        }
    }
    fn push_rebind(&mut self, array: &Expr, element: &Expr) -> Result<(), Diagnostic> {
        // Both arguments finish before the destination gives up its reference.
        // The consumed argument keeps the old allocation alive during the call.
        self.place_address(array)?;
        self.push(); // destination address
        self.bytes(&[0x48, 0x8b, 0x00]);
        self.emit_retain();
        self.push(); // array
        self.str_temp(element)?;
        self.scalar_bits(&element.ty);
        self.push(); // element
        self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 16, 0x48, 0x8b, 0x09]);
        let strings = Self::string_elements(&array.ty);
        self.emit_release(strings);
        self.pop(&[0x5a]);
        self.pop(&[0x59]);
        let amount = self.reserve_call();
        self.bytes(&[0xe8]);
        self.relative(if strings {
            self.rt_push_str
        } else {
            self.rt_push_plain
        });
        self.release_call(amount);
        self.pop(&[0x59]);
        self.bytes(&[0x48, 0x89, 0x01]);
        Ok(())
    }
    fn statement(&mut self, stmt: &Stmt) -> Result<(), Diagnostic> {
        match &stmt.kind {
            StmtKind::Let(name, _, value) => {
                if value.ty.aggregate() {
                    let slot = self.declare(name, value.ty.clone(), &stmt.span)?;
                    return self.store_aggregate(slot, value, false);
                }
                self.str_temp(value)?;
                let slot = self.declare(name, value.ty.clone(), &stmt.span)?;
                if value.ty.single() {
                    self.store_f32(slot);
                } else if value.ty.float() {
                    self.store_f64(slot);
                } else {
                    self.local(0x89, slot);
                }
            }
            StmtKind::Assign(name, value) => {
                let slot = self.variable(name, &stmt.span)?;
                if let ExprKind::Push(array, element) = &value.kind
                    && matches!(&array.kind, ExprKind::Var(source) if source == name)
                {
                    return self.push_rebind(array, element);
                }
                if value.ty.aggregate() {
                    return self.store_aggregate(slot, value, true);
                }
                self.str_temp(value)?;
                if value.ty.owned() {
                    self.push(); // the new value survives the old-value release
                    self.release_slot(
                        slot,
                        value.ty.dyn_array() && value.ty.element().is_some_and(|e| e == Type::Str),
                    );
                    self.pop(&[0x58]);
                }
                if value.ty.single() {
                    self.store_f32(slot);
                } else if value.ty.float() {
                    self.store_f64(slot);
                } else {
                    self.local(0x89, slot);
                }
            }
            StmtKind::IndexAssign(target, value) => {
                if let ExprKind::Push(array, element) = &value.kind
                    && Self::same_storage_path(target, array)
                {
                    return self.push_rebind(array, element);
                }
                if let ExprKind::Index(base, index) = &target.kind
                    && base.ty.dyn_array()
                {
                    // Keep the destination cell separate from the owned array
                    // reference consumed by the runtime. RHS evaluation may call.
                    self.place_address(base)?;
                    self.push(); // destination address
                    self.bytes(&[0x48, 0x8b, 0x00]);
                    self.emit_retain();
                    self.push(); // owned old array
                    self.expr(index)?;
                    self.push();
                    self.str_temp(value)?;
                    self.scalar_bits(&value.ty);
                    self.bytes(&[0x49, 0x89, 0xc0]);
                    self.pop(&[0x5a]);
                    self.pop(&[0x59]);
                    let strings = Self::string_elements(&base.ty);
                    let amount = self.reserve_call();
                    self.bytes(&[0xe8]);
                    self.relative(if strings {
                        self.rt_dset_str
                    } else {
                        self.rt_dset_plain
                    });
                    self.release_call(amount);
                    self.push(); // fresh array
                    self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 8, 0x48, 0x8b, 0x09]);
                    self.emit_release(strings);
                    self.pop(&[0x58]);
                    self.pop(&[0x59]);
                    self.bytes(&[0x48, 0x89, 0x01]);
                    return Ok(());
                }
                self.index_address(target)?;
                self.push();
                if value.ty.aggregate() {
                    self.snapshot(value)?;
                    let slots = value.ty.slots();
                    let mut old = Vec::new();
                    Self::str_offsets(&value.ty.clone(), 0, &mut old);
                    for (string, str_elements) in &old {
                        self.bytes(&[0x48, 0x8b, 0x8c, 0x24]);
                        self.dword((slots * 8) as u32); // mov rcx, [rsp+slots*8]
                        self.bytes(&[0x49, 0x8b, 0x8c, 0x22]);
                        self.dword((string * 8) as u32); // mov rcx, [rcx+string*8]
                        self.emit_release(*str_elements);
                    }
                    self.bytes(&[0x48, 0x8b, 0x8c, 0x24]);
                    self.dword((slots * 8) as u32);
                    for index in 0..slots {
                        self.pop(&[0x58]);
                        self.bytes(&[0x48, 0x89, 0x81]);
                        self.dword((index * 8) as u32);
                    }
                    self.pop(&[0x5a]);
                } else if value.ty.owned() {
                    self.str_temp(value)?;
                    self.push(); // [rsp] = new value, [rsp+8] = address
                    self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 0x08]); // mov rcx, [rsp+8]
                    self.bytes(&[0x48, 0x8b, 0x09]); // mov rcx, [rcx]: the old value
                    self.emit_release(Self::string_elements(&value.ty));
                    self.pop(&[0x58]); // rax = new value
                    self.pop(&[0x59]); // rcx = address
                    self.bytes(&[0x48, 0x89, 0x01]); // mov [rcx], rax
                } else if value.ty.single() {
                    self.expr(value)?;
                    self.pop(&[0x59]);
                    self.bytes(&[0xf3, 0x0f, 0x11, 0x01]); // movss [rcx], xmm0
                } else if value.ty.float() {
                    self.expr(value)?;
                    self.pop(&[0x59]);
                    self.bytes(&[0xf2, 0x0f, 0x11, 0x01]); // movsd [rcx], xmm0
                } else {
                    self.expr(value)?;
                    self.pop(&[0x59]);
                    self.bytes(&[0x48, 0x89, 0x01]);
                }
            }
            StmtKind::Expr(expr) => {
                let initial_stack = self.stack;
                self.expr(expr)?;
                if expr.ty.owned() && !is_place(expr) {
                    self.bytes(&[0x48, 0x89, 0xc1]);
                    self.emit_release(Self::string_elements(&expr.ty));
                }
                self.restore_stack(initial_stack);
            }
            StmtKind::Return(expr) => {
                if self.aggregate_result {
                    self.snapshot(expr)?;
                    self.release_scope(0);
                    self.local(0x8b, 0);
                    self.bytes(&[0x48, 0x89, 0xc1]);
                    for index in 0..expr.ty.slots() {
                        self.pop(&[0x58]);
                        self.bytes(&[0x48, 0x89, 0x81]);
                        self.dword((index * 8) as u32);
                    }
                } else {
                    self.str_temp(expr)?;
                    if expr.ty.float() {
                        self.fpush();
                    } else {
                        self.push();
                    }
                    self.release_scope(0);
                    if expr.ty.float() {
                        self.fpop_load();
                    } else {
                        self.pop(&[0x58]);
                    }
                }
                self.bytes(&[0xc9, 0xc3]);
            }
            StmtKind::Block(body) => self.block(body)?,
            StmtKind::Builtin(kind, expr) => {
                if matches!(kind, Keyword::Print | Keyword::Println) {
                    self.str_temp(expr)?;
                    self.bytes(&[0x50]); // save the temporary
                    self.stack += 1;
                    self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax
                    let amount = self.reserve_call();
                    self.bytes(&[0xe8]);
                    self.relative(self.printstr);
                    self.release_call(amount);
                    if *kind == Keyword::Println {
                        self.bytes(&[0xb9, 10, 0, 0, 0]);
                        let amount = self.reserve_call();
                        self.bytes(&[0xe8]);
                        self.relative(self.printc);
                        self.release_call(amount);
                    }
                    self.pop(&[0x58]); // rax = temporary
                    self.bytes(&[0x48, 0x89, 0xc1]);
                    self.emit_release_rcx();
                    return Ok(());
                }
                self.expr(expr)?;
                if expr.ty.single() {
                    self.bytes(&[0xf3, 0x0f, 0x5a, 0xc0]); // cvtss2sd xmm0, xmm0
                    self.bytes(&[0x66, 0x48, 0x0f, 0x7e, 0xc1]); // movq rcx, xmm0
                } else if expr.ty.float() {
                    self.bytes(&[0x66, 0x48, 0x0f, 0x7e, 0xc1]); // movq rcx, xmm0
                } else {
                    self.bytes(&[0x48, 0x89, 0xc1]);
                }
                let amount = self.reserve_call();
                if *kind == Keyword::Exit {
                    self.bytes(&[0x41, 0xff, 0x57, 0x10]);
                } else {
                    let target = if *kind == Keyword::Printi {
                        if expr.ty.float() {
                            self.printd
                        } else if expr.ty.unsigned() {
                            self.printu
                        } else {
                            self.printi
                        }
                    } else if matches!(kind, Keyword::Print | Keyword::Println) {
                        self.printstr
                    } else {
                        self.printc
                    };
                    self.bytes(&[0xe8]);
                    self.relative(target);
                }
                self.release_call(amount);
                if *kind == Keyword::Println {
                    self.bytes(&[0xb9, 10, 0, 0, 0]);
                    let amount = self.reserve_call();
                    self.bytes(&[0xe8]);
                    self.relative(self.printc);
                    self.release_call(amount);
                }
            }
            StmtKind::If(condition, yes, no) => {
                self.expr(condition)?;
                self.condition_from_float(&condition.ty);
                self.bytes(&[0x48, 0x85, 0xc0]);
                let alternate = self.jump(Some(0x84));
                self.block(yes)?;
                let end = self.jump(None);
                self.here(alternate);
                self.block(no)?;
                self.here(end);
            }
            StmtKind::While(condition, body) => {
                let start = self.code.len();
                self.expr(condition)?;
                self.condition_from_float(&condition.ty);
                self.bytes(&[0x48, 0x85, 0xc0]);
                let end = self.jump(Some(0x84));
                self.loops.push(Loop {
                    start,
                    breaks: Vec::new(),
                    vars: self.vars.len(),
                });
                self.block(body)?;
                let back = self.jump(None);
                self.patch(back, start);
                self.here(end);
                if let Some(loop_) = self.loops.pop() {
                    for at in loop_.breaks {
                        self.here(at);
                    }
                }
            }
            StmtKind::Break => {
                if self.loops.is_empty() {
                    return Err(self.error(&stmt.span, "break is only valid inside a loop"));
                }
                self.release_scope(self.loops.last().expect("checked loop").vars);
                let at = self.jump(None);
                if let Some(loop_) = self.loops.last_mut() {
                    loop_.breaks.push(at);
                }
            }
            StmtKind::Continue => {
                let Some(loop_) = self.loops.last() else {
                    return Err(self.error(&stmt.span, "continue is only valid inside a loop"));
                };
                let vars = loop_.vars;
                let target = loop_.start;
                self.release_scope(vars);
                let at = self.jump(None);
                self.patch(at, target);
            }
        }
        Ok(())
    }
    fn expr(&mut self, expr: &Expr) -> Result<(), Diagnostic> {
        if let Some(value) = constant(expr) {
            self.bytes(&[0x48, 0xb8]);
            self.bytes(&value.to_le_bytes());
            if expr.ty.float() {
                self.bytes(&[0x66, 0x48, 0x0f, 0x6e, 0xc0]); // movq xmm0, rax
            }
            return Ok(());
        }
        match &expr.kind {
            ExprKind::Float(value) => {
                self.bytes(&[0x48, 0xb8]);
                self.bytes(&value.to_bits().to_le_bytes());
            }
            ExprKind::String(value) => self.string_literal(value)?,
            ExprKind::Number(n) => {
                self.bytes(&[0x48, 0xb8]);
                self.bytes(&n.to_le_bytes());
            }
            ExprKind::Array(values) if expr.ty.dyn_array() => {
                self.bytes(&[0xb8]);
                self.dword(values.len() as u32); // mov eax, count
                self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax
                let amount = self.reserve_call();
                self.bytes(&[0xe8]);
                self.relative(self.rt_dnew);
                self.release_call(amount);
                self.push(); // [rsp] = the block
                for (index, element) in values.iter().enumerate() {
                    self.str_temp(element)?;
                    self.scalar_bits(&element.ty);
                    self.bytes(&[0x48, 0x8b, 0x0c, 0x24]); // mov rcx, [rsp]
                    self.bytes(&[0x48, 0x89, 0x81]);
                    self.dword((usize::from(DYN_DATA) + index * 8) as u32); // mov [rcx+payload+i*8], rax
                }
                self.pop(&[0x58]); // rax = the block
            }
            ExprKind::Repeat(value, count) if expr.ty.dyn_array() => {
                self.bytes(&[0xb8]);
                self.dword(*count as u32);
                self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax
                let amount = self.reserve_call();
                self.bytes(&[0xe8]);
                self.relative(self.rt_dnew);
                self.release_call(amount);
                self.push(); // [rsp] = the block
                self.str_temp(value)?;
                self.scalar_bits(&value.ty);
                self.push(); // element, evaluated exactly once
                self.bytes(&[0x31, 0xc0]);
                self.push(); // index
                let fill = self.code.len();
                self.bytes(&[0x48, 0x8b, 0x44, 0x24, 8]);
                if value.ty.owned() {
                    self.emit_retain();
                }
                self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 16]); // block
                self.bytes(&[0x48, 0x8b, 0x14, 0x24]); // index
                self.bytes(&[0x48, 0x89, 0x44, 0xd1, DYN_DATA]);
                self.bytes(&[0x48, 0xff, 0x04, 0x24, 0x48, 0x81, 0x3c, 0x24]);
                self.dword(*count as u32);
                let again = self.jump(Some(0x82));
                self.patch(again, fill);
                self.pop(&[0x5a]);
                self.pop(&[0x59]);
                if value.ty.owned() {
                    self.emit_release_rcx();
                }
                self.pop(&[0x58]); // rax = the block
            }
            ExprKind::Array(_) | ExprKind::Repeat(..) | ExprKind::Record(..) => {
                return Err(self.error(&expr.span, "aggregate literal requires local storage"));
            }
            ExprKind::Field(..) => {
                self.index_address(expr)?;
                if !expr.ty.aggregate() {
                    if expr.ty.single() {
                        self.bytes(&[0xf3, 0x0f, 0x10, 0x00]); // movss xmm0, [rax]
                    } else if expr.ty.float() {
                        self.bytes(&[0xf2, 0x0f, 0x10, 0x00]); // movsd xmm0, [rax]
                    } else {
                        self.bytes(&[0x48, 0x8b, 0x00]);
                    }
                }
            }
            ExprKind::Index(base, _) => {
                self.index_address(expr)?;
                if base.ty == Type::Str || base.ty.dyn_array() {
                    // rt_sbyte/rt_didx already produced the value in rax.
                } else if !expr.ty.aggregate() {
                    if expr.ty.single() {
                        self.bytes(&[0xf3, 0x0f, 0x10, 0x00]); // movss xmm0, [rax]
                    } else if expr.ty.float() {
                        self.bytes(&[0xf2, 0x0f, 0x10, 0x00]); // movsd xmm0, [rax]
                    } else {
                        self.bytes(&[0x48, 0x8b, 0x00]);
                    }
                }
            }
            ExprKind::Push(array, value) => {
                self.str_temp(array)?;
                self.push(); // [rsp] = array
                self.str_temp(value)?;
                self.scalar_bits(&value.ty);
                self.bytes(&[0x48, 0x89, 0xc2]); // mov rdx, rax
                self.bytes(&[0x48, 0x8b, 0x0c, 0x24]); // mov rcx, [rsp]
                let amount = self.reserve_call();
                self.bytes(&[0xe8]);
                self.relative(if array.ty.element().is_some_and(|e| e == Type::Str) {
                    self.rt_push_str
                } else {
                    self.rt_push_plain
                });
                self.release_call(amount);
                // rt_push consumes both references (the array's and the
                // value's) and returns a fresh +1 block: nothing to release.
                self.bytes(&[0x48, 0x83, 0xc4, 0x08]); // add rsp, 8
                self.stack -= 1;
            }
            ExprKind::Len(value) => {
                if value.ty == Type::Str || value.ty.dyn_array() {
                    self.str_temp(value)?;
                    self.push(); // [rsp] = temporary
                    self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax
                    let amount = self.reserve_call();
                    self.bytes(&[0xe8]);
                    self.relative(if value.ty.dyn_array() {
                        self.rt_dlen
                    } else {
                        self.rt_strlen
                    });
                    self.release_call(amount);
                    self.bytes(&[0x48, 0x89, 0x04, 0x24]); // mov [rsp], rax: length
                    self.emit_release(Self::string_elements(&value.ty));
                    self.pop(&[0x58]); // rax = length
                } else {
                    self.expr(value)?; // Preserve index evaluation and bounds errors.
                    self.bytes(&[0xb8]);
                    self.dword(value.ty.len().expect("checked len array") as u32);
                }
            }
            ExprKind::Bool(value) => {
                self.bytes(&[0xb8]);
                self.dword(u32::from(*value));
            }
            ExprKind::Cast(ty, value) => {
                self.expr(value)?;
                if value.ty.float() {
                    let single = value.ty.single();
                    match ty {
                        Type::Bool => self.condition_from_float(&value.ty),
                        Type::F64 => {
                            if single {
                                self.bytes(&[0xf3, 0x0f, 0x5a, 0xc0]); // cvtss2sd xmm0, xmm0
                            }
                        }
                        Type::F32 => {
                            if !single {
                                self.bytes(&[0xf2, 0x0f, 0x5a, 0xc0]); // cvtsd2ss xmm0, xmm0
                            }
                        }
                        // Split at 2^63 so the unsigned range converts exactly.
                        Type::U64 | Type::U32 | Type::U16 | Type::U8 => {
                            if single {
                                self.data_ref(&[0xf3, 0x0f, 0x10, 0x0d], DATA_TWO_POW_63_32);
                                self.bytes(&[0x0f, 0x2e, 0xc1]); // ucomiss xmm0, xmm1
                                let below = self.jump(Some(0x82)); // jb
                                self.bytes(&[0xf3, 0x0f, 0x5c, 0xc1]); // subss xmm0, xmm1
                                self.bytes(&[0xf3, 0x48, 0x0f, 0x2c, 0xc0]); // cvttss2si rax
                                self.bytes(&[0x48, 0xba]);
                                self.bytes(&0x8000000000000000u64.to_le_bytes()); // mov rdx, 2^63
                                self.bytes(&[0x48, 0x01, 0xd0]); // add rax, rdx
                                let done = self.jump(None);
                                self.here(below);
                                self.bytes(&[0xf3, 0x48, 0x0f, 0x2c, 0xc0]); // cvttss2si rax
                                self.here(done);
                            } else {
                                self.data_ref(&[0xf2, 0x0f, 0x10, 0x0d], DATA_TWO_POW_63);
                                self.bytes(&[0x66, 0x0f, 0x2e, 0xc1]); // ucomisd xmm0, xmm1
                                let below = self.jump(Some(0x82)); // jb
                                self.bytes(&[0xf2, 0x0f, 0x5c, 0xc1]); // subsd xmm0, xmm1
                                self.bytes(&[0xf2, 0x48, 0x0f, 0x2c, 0xc0]); // cvttsd2si rax
                                self.bytes(&[0x48, 0xba]);
                                self.bytes(&0x8000000000000000u64.to_le_bytes()); // mov rdx, 2^63
                                self.bytes(&[0x48, 0x01, 0xd0]); // add rax, rdx
                                let done = self.jump(None);
                                self.here(below);
                                self.bytes(&[0xf2, 0x48, 0x0f, 0x2c, 0xc0]); // cvttsd2si rax
                                self.here(done);
                            }
                        }
                        _ => {
                            if single {
                                self.bytes(&[0xf3, 0x48, 0x0f, 0x2c, 0xc0]); // cvttss2si rax
                            } else {
                                self.bytes(&[0xf2, 0x48, 0x0f, 0x2c, 0xc0]); // cvttsd2si rax
                            }
                        }
                    }
                    self.canonical(ty);
                } else if ty.float() {
                    if value.ty == Type::U64 {
                        self.bytes(&[0x48, 0x85, 0xc0]); // test rax, rax
                        let small = self.jump(Some(0x89)); // jns
                        self.bytes(&[0x48, 0x89, 0xc2]); // mov rdx, rax
                        self.bytes(&[0x48, 0xd1, 0xea]); // shr rdx, 1
                        self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax
                        self.bytes(&[0x83, 0xe1, 0x01]); // and ecx, 1
                        if ty.single() {
                            self.bytes(&[0xf3, 0x48, 0x0f, 0x2a, 0xc2]); // cvtsi2ss xmm0, rdx
                            self.bytes(&[0xf3, 0x0f, 0x58, 0xc0]); // addss xmm0, xmm0: *2 exact
                            self.bytes(&[0xf3, 0x48, 0x0f, 0x2a, 0xc9]); // cvtsi2ss xmm1, rcx
                            self.bytes(&[0xf3, 0x0f, 0x58, 0xc1]); // addss xmm0, xmm1
                        } else {
                            self.bytes(&[0xf2, 0x48, 0x0f, 0x2a, 0xc2]); // cvtsi2sd xmm0, rdx
                            self.bytes(&[0xf2, 0x48, 0x0f, 0x2a, 0xc9]); // cvtsi2sd xmm1, rcx
                            self.bytes(&[0xf2, 0x0f, 0x58, 0xc1]); // addsd xmm0, xmm1
                        }
                        let done = self.jump(None);
                        self.here(small);
                        if ty.single() {
                            self.bytes(&[0xf3, 0x48, 0x0f, 0x2a, 0xc0]); // cvtsi2ss xmm0, rax
                        } else {
                            self.bytes(&[0xf2, 0x48, 0x0f, 0x2a, 0xc0]); // cvtsi2sd xmm0, rax
                        }
                        self.here(done);
                    } else if ty.single() {
                        self.bytes(&[0xf3, 0x48, 0x0f, 0x2a, 0xc0]); // cvtsi2ss xmm0, rax
                    } else {
                        self.bytes(&[0xf2, 0x48, 0x0f, 0x2a, 0xc0]); // cvtsi2sd xmm0, rax
                    }
                } else {
                    match ty {
                        Type::Bool => self
                            .bytes(&[0x48, 0x85, 0xc0, 0x0f, 0x95, 0xc0, 0x48, 0x0f, 0xb6, 0xc0]),
                        // Conversions keep the low bits: truncate, then restore
                        // the canonical form of the target type.
                        Type::I32 => self.bytes(&[0x48, 0x63, 0xc0]),
                        Type::U32 => self.bytes(&[0x89, 0xc0]),
                        Type::I16 => self.bytes(&[0x48, 0x0f, 0xbf, 0xc0]),
                        Type::U16 => self.bytes(&[0x48, 0x0f, 0xb7, 0xc0]),
                        Type::I8 => self.bytes(&[0x48, 0x0f, 0xbe, 0xc0]),
                        Type::U8 => self.bytes(&[0x48, 0x0f, 0xb6, 0xc0]),
                        _ => {}
                    }
                }
            }
            ExprKind::Var(name) => {
                let slot = self.variable(name, &expr.span)?;
                if expr.ty.aggregate() {
                    self.local(0x8d, slot + expr.ty.slots() - 1);
                } else if expr.ty.single() {
                    self.load_f32(slot);
                } else if expr.ty.float() {
                    self.load_f64(slot);
                } else {
                    self.local(0x8b, slot);
                }
            }
            ExprKind::Unary(op, value) => {
                self.expr(value)?;
                match *op {
                    "-" => {
                        if value.ty.single() {
                            self.data_ref(&[0xf3, 0x0f, 0x10, 0x0d], DATA_SIGN_32);
                            self.bytes(&[0x0f, 0x57, 0xc1]); // xorps xmm0, xmm1
                        } else if value.ty.float() {
                            // Sign-bit flip, matching the constant folder and
                            // IEEE negation exactly, including -0.0 and NaN.
                            self.data_ref(&[0xf2, 0x0f, 0x10, 0x0d], DATA_SIGN);
                            self.bytes(&[0x66, 0x0f, 0x57, 0xc1]); // xorpd xmm0, xmm1
                        } else {
                            self.bytes(&[0x48, 0xf7, 0xd8]);
                            self.canonical(&expr.ty);
                        }
                    }
                    "~" => {
                        self.bytes(&[0x48, 0xf7, 0xd0]);
                        self.canonical(&expr.ty);
                    }
                    "!" => {
                        if value.ty.float() {
                            self.bytes(&[0x66, 0x0f, 0xef, 0xc9]); // pxor xmm1, xmm1
                            self.bytes(&[0x66, 0x0f, 0x2e, 0xc1]); // ucomisd xmm0, 0
                            self.bytes(&[0x0f, 0x94, 0xc0]); // sete al
                            self.bytes(&[0x0f, 0x9b, 0xc1]); // setnp cl
                            self.bytes(&[0x22, 0xc1]); // and al, cl
                            self.bytes(&[0x48, 0x0f, 0xb6, 0xc0]);
                        } else {
                            self.bytes(&[
                                0x48, 0x85, 0xc0, 0x0f, 0x94, 0xc0, 0x48, 0x0f, 0xb6, 0xc0,
                            ])
                        }
                    }
                    _ => {}
                }
            }
            ExprKind::Call(name, args) => {
                let Some(&(index, arity)) = self.functions.get(name) else {
                    return Err(self.error(&expr.span, format!("undefined function: {name}")));
                };
                if args.len() != arity {
                    return Err(self.error(
                        &expr.span,
                        format!("wrong number of function arguments for {name}: expected {arity}"),
                    ));
                }
                let initial_stack = self.stack;
                if expr.ty.aggregate() {
                    self.bytes(&[0x31, 0xc0]);
                    for _ in 0..expr.ty.slots() {
                        self.push();
                    }
                }
                let result_stack = self.stack;
                let mut locations = Vec::with_capacity(args.len());
                let aggregates = args.iter().any(|arg| arg.ty.aggregate());
                let plan: Vec<(bool, usize)> = args
                    .iter()
                    .take(4)
                    .enumerate()
                    .map(|(position, arg)| (arg.ty.float(), position))
                    .collect();
                for arg in args {
                    if arg.ty.aggregate() {
                        self.snapshot(arg)?;
                        self.bytes(&[0x48, 0x89, 0xe0]);
                    } else {
                        self.str_temp(arg)?;
                    }
                    if arg.ty.float() {
                        self.fpush();
                    } else {
                        self.push();
                    }
                    locations.push(self.stack);
                }
                if args.len() > 4 || aggregates || expr.ty.aggregate() {
                    // Preserve evaluated arguments above a separate outgoing
                    // call area. Nested calls cannot overwrite these values.
                    let base = 32 + args.len().saturating_sub(4) * 8;
                    let amount = base + ((self.stack * 8 + base) % 16);
                    self.bytes(&[0x48, 0x81, 0xec]);
                    self.dword(amount as u32);
                    // Stack arguments first: they shuttle through xmm0/r10,
                    // which would clobber already-loaded register arguments.
                    for (arg, location) in locations.iter().enumerate().skip(4) {
                        let source = amount + (self.stack - location) * 8;
                        let slot = 32 + (arg - 4) * 8;
                        if args[arg].ty.float() {
                            self.bytes(&[0xf2, 0x0f, 0x10, 0x84, 0x24]);
                            self.dword(source as u32);
                            self.bytes(&[0xf2, 0x0f, 0x11, 0x84, 0x24]);
                            self.dword(slot as u32);
                        } else {
                            self.bytes(&[0x4c, 0x8b, 0x94, 0x24]);
                            self.dword(source as u32);
                            self.bytes(&[0x4c, 0x89, 0x94, 0x24]);
                            self.dword(slot as u32);
                        }
                    }
                    for (arg, location) in locations.iter().enumerate().take(4) {
                        let source = amount + (self.stack - location) * 8;
                        if args[arg].ty.float() {
                            let xmm = plan[arg].1;
                            self.bytes(&[0xf2, 0x0f, 0x10, 0x84 | ((xmm as u8) << 3), 0x24]);
                            self.dword(source as u32);
                        } else {
                            self.bytes(
                                &[
                                    [0x48, 0x8b, 0x8c, 0x24],
                                    [0x48, 0x8b, 0x94, 0x24],
                                    [0x4c, 0x8b, 0x84, 0x24],
                                    [0x4c, 0x8b, 0x8c, 0x24],
                                ][plan[arg].1],
                            );
                            self.dword(source as u32);
                        }
                    }
                    if expr.ty.aggregate() {
                        self.bytes(&[0x4c, 0x8d, 0x94, 0x24]);
                        self.dword((amount + (self.stack - result_stack) * 8) as u32);
                    }
                    self.bytes(&[0xe8]);
                    let at = self.hole();
                    self.calls.push((at, index));
                    // Argument snapshots keep their string references until
                    // the call returns; release them before the stack pop.
                    // The result is saved first: rt_release clobbers rax.
                    if expr.ty.float() {
                        self.fpush();
                    } else {
                        self.push();
                    }
                    for (arg, location) in locations.iter().enumerate() {
                        if !args[arg].ty.aggregate() {
                            continue;
                        }
                        let base = amount + (self.stack - location) * 8;
                        let mut owned = Vec::new();
                        Self::str_offsets(&args[arg].ty.clone(), 0, &mut owned);
                        for (string, str_elements) in owned {
                            self.bytes(&[0x4c, 0x8b, 0x94, 0x24]);
                            self.dword(base as u32); // reload after each release
                            self.bytes(&[0x49, 0x8b, 0x8c, 0x22]);
                            self.dword((string * 8) as u32); // mov rcx, [r10+string*8]
                            self.emit_release(str_elements);
                        }
                    }
                    if expr.ty.float() {
                        self.fpop_load();
                    } else {
                        self.pop(&[0x58]);
                    }
                    self.bytes(&[0x48, 0x81, 0xc4]);
                    self.dword((amount + (self.stack - result_stack) * 8) as u32);
                    self.stack = result_stack;
                    if expr.ty.aggregate() {
                        self.bytes(&[0x48, 0x89, 0xe0]);
                    }
                    debug_assert!(self.stack >= initial_stack);
                    return Ok(());
                }
                for arg in (0..args.len()).rev() {
                    let (is_float, index) = plan[arg];
                    if is_float {
                        self.bytes(&[0xf2, 0x0f, 0x10, 0x04 | ((index as u8) << 3), 0x24]);
                        self.bytes(&[0x48, 0x83, 0xc4, 0x08]);
                        self.stack -= 1;
                    } else {
                        self.pop([&[0x59][..], &[0x5a], &[0x41, 0x58], &[0x41, 0x59]][index]);
                    }
                }
                let amount = self.reserve_call();
                self.bytes(&[0xe8]);
                let at = self.hole();
                self.calls.push((at, index));
                self.release_call(amount);
            }
            ExprKind::Binary(op, left, right) if *op == "&&" || *op == "||" => {
                self.expr(left)?;
                self.condition_from_float(&left.ty);
                self.bytes(&[0x48, 0x85, 0xc0]);
                let skip = self.jump(Some(if *op == "&&" { 0x84 } else { 0x85 }));
                self.expr(right)?;
                self.condition_from_float(&right.ty);
                self.bytes(&[0x48, 0x85, 0xc0]);
                self.here(skip);
                // Both paths arrive with flags from their own test instruction.
                self.bytes(&[0x0f, 0x95, 0xc0, 0x48, 0x0f, 0xb6, 0xc0]);
            }
            // Comparisons return bool, so the operand type drives the float
            // path; semantics keeps operands homogeneous.
            ExprKind::Binary(op, left, right) if left.ty.float() => {
                let single = left.ty.single();
                let (add, sub, mul, div) = if single {
                    (
                        [0xf3, 0x0f, 0x58, 0xc1],
                        [0xf3, 0x0f, 0x5c, 0xc1],
                        [0xf3, 0x0f, 0x59, 0xc1],
                        [0xf3, 0x0f, 0x5e, 0xc1],
                    )
                } else {
                    (
                        [0xf2, 0x0f, 0x58, 0xc1],
                        [0xf2, 0x0f, 0x5c, 0xc1],
                        [0xf2, 0x0f, 0x59, 0xc1],
                        [0xf2, 0x0f, 0x5e, 0xc1],
                    )
                };
                self.expr(left)?;
                self.fpush();
                self.expr(right)?;
                self.bytes(&[0xf2, 0x0f, 0x10, 0xc8]); // movsd xmm1, xmm0 (load form)
                self.fpop_load();
                match *op {
                    "+" => self.bytes(&add),
                    "-" => self.bytes(&sub),
                    "*" => self.bytes(&mul),
                    "/" => self.bytes(&div),
                    "==" | "!=" | "<" | ">" | "<=" | ">=" => {
                        // ucomiss has no 66 prefix; ucomisd does.
                        if single {
                            self.bytes(&[0x0f, 0x2e, 0xc1]); // ucomiss xmm0, xmm1
                        } else {
                            self.bytes(&[0x66, 0x0f, 0x2e, 0xc1]); // ucomisd xmm0, xmm1
                        }
                        let swapped = matches!(*op, "<" | "<=");
                        if swapped {
                            // Re-compare with operands exchanged: unordered and
                            // equal cases keep the intended strict semantics.
                            if single {
                                self.bytes(&[0x0f, 0x2e, 0xc8]); // ucomiss xmm1, xmm0
                            } else {
                                self.bytes(&[0x66, 0x0f, 0x2e, 0xc8]); // ucomisd xmm1, xmm0
                            }
                        }
                        match *op {
                            "==" => {
                                self.bytes(&[0x0f, 0x94, 0xc0]); // sete al
                                self.bytes(&[0x0f, 0x9b, 0xc1]); // setnp cl
                                self.bytes(&[0x22, 0xc1]); // and al, cl
                            }
                            "!=" => {
                                self.bytes(&[0x0f, 0x95, 0xc0]); // setne al
                                self.bytes(&[0x0f, 0x9a, 0xc1]); // setp cl
                                self.bytes(&[0x08, 0xc1]); // or al, cl
                            }
                            "<" | ">" => {
                                self.bytes(&[0x0f, 0x97, 0xc0]); // seta al
                            }
                            _ => {
                                self.bytes(&[0x0f, 0x93, 0xc0]); // setae al
                            }
                        }
                        self.bytes(&[0x48, 0x0f, 0xb6, 0xc0]); // movzx rax, al
                    }
                    _ => return Err(self.error(&expr.span, "unsupported float operator")),
                }
                return Ok(());
            }
            ExprKind::Binary(op, left, right) => {
                if left.ty == Type::Str {
                    self.str_temp(left)?;
                    self.push();
                    self.str_temp(right)?;
                    self.push();
                    // [rsp] = right, [rsp+8] = left.
                    self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 0x08]); // mov rcx, [rsp+8]
                    self.bytes(&[0x48, 0x8b, 0x14, 0x24]); // mov rdx, [rsp]
                    let amount = self.reserve_call();
                    self.bytes(&[0xe8]);
                    self.relative(if *op == "+" {
                        self.rt_concat
                    } else {
                        self.streq
                    });
                    self.release_call(amount);
                    self.push(); // push rax: the result
                    self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 0x08]); // mov rcx, [rsp+8]
                    self.emit_release_rcx();
                    self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 0x10]); // mov rcx, [rsp+16]
                    self.emit_release_rcx();
                    self.pop(&[0x58]); // rax = the result (releases clobber rax)
                    self.bytes(&[0x48, 0x83, 0xc4, 0x10]); // add rsp, 10h
                    self.stack -= 2;
                    if *op == "!=" {
                        self.bytes(&[0x83, 0xf0, 1]);
                    }
                    return Ok(());
                }
                self.expr(left)?;
                self.push();
                self.expr(right)?;
                self.bytes(&[0x48, 0x89, 0xc1]);
                self.pop(&[0x58]);
                match *op {
                    "+" => {
                        self.bytes(&[0x48, 0x01, 0xc8]);
                        self.canonical(&expr.ty);
                    }
                    "-" => {
                        self.bytes(&[0x48, 0x29, 0xc8]);
                        self.canonical(&expr.ty);
                    }
                    "*" => {
                        self.bytes(&[0x48, 0x0f, 0xaf, 0xc1]);
                        self.canonical(&expr.ty);
                    }
                    "&" => self.bytes(&[0x48, 0x21, 0xc8]),
                    "|" => self.bytes(&[0x48, 0x09, 0xc8]),
                    "^" => self.bytes(&[0x48, 0x31, 0xc8]),
                    "<<" => {
                        if expr.ty.narrow() {
                            self.narrow_shift(&expr.ty, 0xe0);
                        } else {
                            self.bytes(&[0x48, 0xd3, 0xe0]);
                        }
                    }
                    ">>" => {
                        if expr.ty.narrow() {
                            self.narrow_shift(
                                &expr.ty,
                                if left.ty.unsigned() { 0xe8 } else { 0xf8 },
                            );
                        } else {
                            self.bytes(&[
                                0x48,
                                0xd3,
                                if left.ty == Type::U64 { 0xe8 } else { 0xf8 },
                            ]);
                        }
                    }
                    ">>>" => {
                        if expr.ty.narrow() {
                            self.narrow_shift(&expr.ty, 0xe8);
                        } else {
                            self.bytes(&[0x48, 0xd3, 0xe8]);
                        }
                    }
                    "/" | "%" => {
                        self.bytes(&[0x48, 0x85, 0xc9]);
                        let nonzero = self.jump(Some(0x85));
                        self.exit(70);
                        self.here(nonzero);
                        if left.ty.unsigned() {
                            // Canonical u32/u64 operands cannot fault and the
                            // quotient always fits the unsigned width.
                            self.bytes(&[0x31, 0xd2, 0x48, 0xf7, 0xf1]);
                            if *op == "%" {
                                self.bytes(&[0x48, 0x89, 0xd0]);
                            }
                            return Ok(());
                        }
                        if left.ty.narrow() {
                            // Canonical signed operands cannot fault; only the
                            // quotient may leave the type range (MIN / -1).
                            self.bytes(&[0x48, 0x99, 0x48, 0xf7, 0xf9]);
                            if *op == "%" {
                                // The remainder always fits the width and is
                                // already sign-extended by idiv.
                                self.bytes(&[0x48, 0x89, 0xd0]);
                                return Ok(());
                            }
                            match left.ty {
                                Type::I32 => self.bytes(&[0x48, 0x63, 0xd0]), // movsxd rdx, eax
                                Type::I16 => self.bytes(&[0x48, 0x0f, 0xbf, 0xd0]), // movsx rdx, ax
                                _ => self.bytes(&[0x48, 0x0f, 0xbe, 0xd0]),   // movsx rdx, al
                            }
                            self.bytes(&[0x48, 0x39, 0xd0]); // cmp rdx, rax
                            let in_range = self.jump(Some(0x84));
                            self.exit(71);
                            self.here(in_range);
                            self.bytes(&[0x48, 0x89, 0xd0]); // mov rax, rdx
                            return Ok(());
                        }
                        self.bytes(&[0x48, 0xba]);
                        self.bytes(&i64::MIN.to_le_bytes());
                        self.bytes(&[0x48, 0x39, 0xd0]);
                        let normal = self.jump(Some(0x85));
                        self.bytes(&[0x48, 0x83, 0xf9, 0xff]);
                        let normal2 = self.jump(Some(0x85));
                        self.exit(71);
                        self.here(normal);
                        self.here(normal2);
                        self.bytes(&[0x48, 0x99, 0x48, 0xf7, 0xf9]);
                        if *op == "%" {
                            self.bytes(&[0x48, 0x89, 0xd0]);
                        }
                    }
                    _ => {
                        let condition = match *op {
                            "==" => 0x94,
                            "!=" => 0x95,
                            "<" => {
                                if left.ty.unsigned() {
                                    0x92
                                } else {
                                    0x9c
                                }
                            }
                            ">" => {
                                if left.ty.unsigned() {
                                    0x97
                                } else {
                                    0x9f
                                }
                            }
                            "<=" => {
                                if left.ty.unsigned() {
                                    0x96
                                } else {
                                    0x9e
                                }
                            }
                            ">=" => {
                                if left.ty.unsigned() {
                                    0x93
                                } else {
                                    0x9d
                                }
                            }
                            _ => return Err(self.error(&expr.span, "unsupported operator")),
                        };
                        self.bytes(&[
                            0x48, 0x39, 0xc8, 0x0f, condition, 0xc0, 0x48, 0x0f, 0xb6, 0xc0,
                        ]);
                    }
                }
            }
        }
        Ok(())
    }
    fn runtime(&mut self) -> usize {
        self.bytes(&[0x48, 0x83, 0xec, 0x28, 0x4c, 0x8d, 0x3d]);
        self.dword(IMPORTS + 0x40 - (TEXT + self.code.len() as u32 + 4));
        self.bytes(&[0xe8]);
        let main = self.hole();
        self.bytes(&[0x89, 0xc1, 0x41, 0xff, 0x57, 0x10, 0xcc]);
        self.printi = self.code.len();
        self.bytes(&[0xba, 1, 0, 0, 0]);
        let signed_entry = self.jump(None);
        self.printu = self.code.len();
        self.bytes(&[0x31, 0xd2]);
        self.here(signed_entry);
        self.bytes(&[
            0x53, 0x48, 0x83, 0xec, 0x60, 0x48, 0x89, 0xcb, 0x48, 0x89, 0x54, 0x24, 0x28, 0xb9,
            0xf5, 0xff, 0xff, 0xff, 0x41, 0xff, 0x17, 0x48, 0x89, 0x44, 0x24, 0x30, 0x4c, 0x8d,
            0x54, 0x24, 0x60, 0x49, 0xff, 0xca, 0x41, 0xc6, 0x02, 0x0a, 0x48, 0x89, 0xd8,
        ]);
        self.bytes(&[0x48, 0x83, 0x7c, 0x24, 0x28, 0]);
        let unsigned_value = self.jump(Some(0x84));
        self.bytes(&[0x48, 0x85, 0xc0]);
        let positive = self.jump(Some(0x89));
        self.bytes(&[0x48, 0xf7, 0xd8]);
        self.here(positive);
        self.here(unsigned_value);
        self.bytes(&[0xb9, 10, 0, 0, 0]);
        let digit = self.code.len();
        self.bytes(&[
            0x31, 0xd2, 0x48, 0xf7, 0xf1, 0x80, 0xc2, 0x30, 0x49, 0xff, 0xca, 0x41, 0x88, 0x12,
            0x48, 0x85, 0xc0,
        ]);
        let again = self.jump(Some(0x85));
        self.patch(again, digit);
        self.bytes(&[0x48, 0x83, 0x7c, 0x24, 0x28, 0]);
        let unsigned_prefix = self.jump(Some(0x84));
        self.bytes(&[0x48, 0x85, 0xdb]);
        let unsigned = self.jump(Some(0x89));
        self.bytes(&[0x49, 0xff, 0xca, 0x41, 0xc6, 0x02, 0x2d]);
        self.here(unsigned);
        self.here(unsigned_prefix);
        self.bytes(&[
            0x48, 0x8b, 0x4c, 0x24, 0x30, 0x4c, 0x89, 0xd2, 0x4c, 0x8d, 0x44, 0x24, 0x60, 0x4d,
            0x29, 0xd0, 0x4c, 0x8d, 0x4c, 0x24, 0x38, 0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0,
            0x41, 0xff, 0x57, 8, 0x48, 0x83, 0xc4, 0x60, 0x5b, 0xc3,
        ]);
        self.printc = self.code.len();
        self.bytes(&[
            0x48, 0x83, 0xec, 0x48, 0x88, 0x4c, 0x24, 0x30, 0xb9, 0xf5, 0xff, 0xff, 0xff, 0x41,
            0xff, 0x17, 0x48, 0x89, 0xc1, 0x48, 0x8d, 0x54, 0x24, 0x30, 0x41, 0xb8, 1, 0, 0, 0,
            0x4c, 0x8d, 0x4c, 0x24, 0x38, 0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0, 0x41, 0xff,
            0x57, 8, 0x48, 0x83, 0xc4, 0x48, 0xc3,
        ]);
        self.printstr = self.code.len();
        self.bytes(&[0x48, 0x83, 0xec, 0x48]); // sub rsp, 48h
        self.bytes(&[0x48, 0x89, 0x4c, 0x24, 0x30]); // mov [rsp+30h], rcx
        self.bytes(&[0xb9, 0xf5, 0xff, 0xff, 0xff]); // mov ecx, -11
        self.bytes(&[0x41, 0xff, 0x17]); // call GetStdHandle
        self.bytes(&[0x48, 0x8b, 0x54, 0x24, 0x30]); // mov rdx, [rsp+30h]
        self.bytes(&[0xf6, 0xc2, 0x01]); // test dl, 1
        let printstr_i = self.jump(Some(0x84));
        self.bytes(&[0x4c, 0x8b, 0x42, 0x07]); // mov r8, [rdx+7]
        self.bytes(&[0x48, 0x8d, 0x52, 0x0f]); // lea rdx, [rdx+15]
        let printstr_h = self.jump(None);
        self.here(printstr_i);
        self.bytes(&[0x4c, 0x8b, 0x02]); // mov r8, [rdx]
        self.bytes(&[0x48, 0x83, 0xc2, 0x08]); // add rdx, 8
        self.here(printstr_h);
        self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax
        self.bytes(&[0x4c, 0x8d, 0x4c, 0x24, 0x38]); // lea r9, [rsp+38h]
        self.bytes(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
        self.bytes(&[0x41, 0xff, 0x57, 0x08]); // call WriteFile
        self.bytes(&[0x48, 0x83, 0xc4, 0x48]);
        self.bytes(&[0xc3]);
        self.streq = self.code.len();
        // r9 = len a, rcx = bytes a.
        self.bytes(&[0x4c, 0x8b, 0x09]); // mov r9, [rcx]
        self.bytes(&[0xf6, 0xc1, 0x01]); // test cl, 1
        let sa_i = self.jump(Some(0x84));
        self.bytes(&[0x4c, 0x8b, 0x49, 0x07]); // mov r9, [rcx+7]
        self.bytes(&[0x48, 0x8d, 0x49, 0x0f]); // lea rcx, [rcx+15]
        let sa_h = self.jump(None);
        self.here(sa_i);
        self.bytes(&[0x48, 0x8d, 0x49, 0x08]); // lea rcx, [rcx+8]
        self.here(sa_h);
        // r10 = len b, rdx = bytes b.
        self.bytes(&[0x4c, 0x8b, 0x12]); // mov r10, [rdx]
        self.bytes(&[0xf6, 0xc2, 0x01]); // test dl, 1
        let sb_i = self.jump(Some(0x84));
        self.bytes(&[0x4c, 0x8b, 0x52, 0x07]); // mov r10, [rdx+7]
        self.bytes(&[0x48, 0x8d, 0x52, 0x0f]); // lea rdx, [rdx+15]
        let sb_h = self.jump(None);
        self.here(sb_i);
        self.bytes(&[0x48, 0x83, 0xc2, 0x08]); // add rdx, 8
        self.here(sb_h);
        self.bytes(&[0x4d, 0x39, 0xd1]); // cmp r9, r10
        let unequal_length = self.jump(Some(0x85));
        self.bytes(&[0x31, 0xc0]); // xor eax, eax
        let compare = self.code.len();
        self.bytes(&[0x4c, 0x39, 0xc8]); // cmp rax, r9
        let equal = self.jump(Some(0x84));
        self.bytes(&[0x44, 0x8a, 0x14, 0x01]); // mov r10b, [rcx+rax]
        self.bytes(&[0x44, 0x3a, 0x14, 0x02]); // cmp r10b, [rdx+rax]
        let unequal_byte = self.jump(Some(0x85));
        self.bytes(&[0x48, 0xff, 0xc0]); // inc rax
        let next = self.jump(None);
        self.patch(next, compare);
        self.here(equal);
        self.bytes(&[0xb8, 1, 0, 0, 0, 0xc3]);
        self.here(unequal_length);
        self.here(unequal_byte);
        self.bytes(&[0x31, 0xc0, 0xc3]);
        // Heap runtime. A str value is an 8-byte pointer; bit 0 tags the
        // kind: 0 = interned read-only data ([len][bytes]), 1 = heap block
        // (ptr-1) with a reference-count header ([rc][len][bytes]).
        self.rt_alloc = self.code.len();
        // rcx = size. Preserve it in rbx across GetProcessHeap.
        self.bytes(&[0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x89, 0xcb]);
        self.bytes(&[0x48, 0x81, 0xf9]); // cmp rcx, allocation budget
        self.dword(64 * 1024 * 1024);
        let alloc_size_ok = self.jump(Some(0x86));
        self.bytes(&[0xb9]);
        self.dword(73);
        self.bytes(&[0x41, 0xff, 0x57, 0x10]);
        self.here(alloc_size_ok);
        self.bytes(&[0x41, 0xff, 0x57, 0x18]);
        self.bytes(&[0xba, 8, 0, 0, 0, 0x49, 0x89, 0xd8, 0x48, 0x89, 0xc1]);
        self.bytes(&[0x41, 0xff, 0x57, 0x20, 0x48, 0x85, 0xc0]);
        let alloc_ok = self.jump(Some(0x85));
        self.bytes(&[0xb9]);
        self.dword(74); // deterministic allocation failure, no null dereference
        self.bytes(&[0x41, 0xff, 0x57, 0x10]);
        self.here(alloc_ok);
        self.bytes(&[0x48, 0x83, 0xc4, 0x20, 0x5b, 0xc3]);
        self.rt_retain = self.code.len();
        // rcx = value -> +1 reference for heap values, interned stays put.
        self.bytes(&[0xf6, 0xc1, 0x01]); // test cl, 1
        let retain_done = self.jump(Some(0x84));
        self.bytes(&[0x48, 0xff, 0x41, 0xff]); // inc qword [rcx-1]
        self.here(retain_done);
        self.bytes(&[0xc3]);
        self.rt_release = self.code.len();
        // rcx = value -> -1 reference; frees the block at zero.
        self.bytes(&[0x53]); // push rbx
        self.bytes(&[0xf6, 0xc1, 0x01]); // test cl, 1
        let release_done = self.jump(Some(0x84));
        self.bytes(&[0x48, 0x8d, 0x59, 0xff]); // lea rbx, [rcx-1]
        self.bytes(&[0x48, 0xff, 0x0b]); // dec qword [rbx]
        let release_live = self.jump(Some(0x85));
        self.bytes(&[0x48, 0x83, 0xec, 0x20]); // sub rsp, 0x28
        self.bytes(&[0x41, 0xff, 0x57, 0x18]); // call GetProcessHeap
        self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax (heap)
        self.bytes(&[0x31, 0xd2]); // xor edx, edx (flags)
        self.bytes(&[0x49, 0x89, 0xd8]); // mov r8, rbx (block)
        self.bytes(&[0x45, 0x31, 0xc9]); // xor r9d, r9d
        self.bytes(&[0x41, 0xff, 0x57, 0x28]); // call HeapFree
        self.bytes(&[0x48, 0x83, 0xc4, 0x20]);
        self.here(release_live);
        self.here(release_done);
        self.bytes(&[0x5b]); // pop rbx
        self.bytes(&[0xc3]);
        self.rt_concat = self.code.len();
        // rcx = a, rdx = b -> rax = fresh heap value with refcount 1.
        self.bytes(&[0x53, 0x56, 0x57]); // save rbx, rsi, rdi
        self.bytes(&[0x48, 0x83, 0xec, 0x40]); // sub rsp, 0x40
        self.bytes(&[0x48, 0x89, 0x4c, 0x24, 0x20]); // mov [rsp+20h], rcx
        self.bytes(&[0x48, 0x89, 0x54, 0x24, 0x28]); // mov [rsp+28h], rdx
        self.bytes(&[0x48, 0x89, 0xc8]); // mov rax, rcx
        self.bytes(&[0xa8, 0x01]); // test al, 1
        let concat_ia = self.jump(Some(0x84));
        self.bytes(&[0x48, 0x8b, 0x40, 0x07]); // mov rax, [rax+7]
        let concat_ha = self.jump(None);
        self.here(concat_ia);
        self.bytes(&[0x48, 0x8b, 0x00]); // mov rax, [rax]
        self.here(concat_ha);
        self.bytes(&[0x48, 0x89, 0x44, 0x24, 0x30]); // mov [rsp+30h], rax
        self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 0x28]); // mov rcx, [rsp+28h]
        self.bytes(&[0xf6, 0xc1, 0x01]); // test cl, 1
        let concat_ib = self.jump(Some(0x84));
        self.bytes(&[0x48, 0x8b, 0x49, 0x07]); // mov rcx, [rcx+7]
        let concat_hb = self.jump(None);
        self.here(concat_ib);
        self.bytes(&[0x48, 0x8b, 0x09]); // mov rcx, [rcx]
        self.here(concat_hb);
        self.bytes(&[0x48, 0x01, 0xc8]); // add rax, rcx
        self.bytes(&[0x48, 0x89, 0x44, 0x24, 0x38]); // mov [rsp+38h], rax
        self.bytes(&[0x48, 0x8d, 0x48, 0x10]); // lea rcx, [rax+10h]
        self.bytes(&[0xe8]);
        let concat_alloc = self.hole();
        self.patch(concat_alloc, self.rt_alloc);
        self.bytes(&[0x48, 0x89, 0xc3]); // mov rbx, rax
        self.bytes(&[0x48, 0xc7, 0x03, 1, 0, 0, 0]); // mov qword [rbx], 1
        self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 0x38]);
        self.bytes(&[0x48, 0x89, 0x4b, 0x08]); // mov [rbx+8], rcx
        self.bytes(&[0x48, 0x8d, 0x7b, 0x10]); // lea rdi, [rbx+10h]
        self.bytes(&[0x48, 0x8b, 0x44, 0x24, 0x20]); // mov rax, [rsp+20h]
        self.bytes(&[0xa8, 0x01]); // test al, 1
        let concat_ca = self.jump(Some(0x84));
        self.bytes(&[0x48, 0x8b, 0x48, 0x07]); // mov rcx, [rax+7]
        self.bytes(&[0x48, 0x8d, 0x70, 0x0f]); // lea rsi, [rax+15]
        let concat_da = self.jump(None);
        self.here(concat_ca);
        self.bytes(&[0x48, 0x8b, 0x08]); // mov rcx, [rax]
        self.bytes(&[0x48, 0x8d, 0x70, 0x08]); // lea rsi, [rax+8]
        self.here(concat_da);
        self.bytes(&[0xf3, 0xa4]); // rep movsb
        self.bytes(&[0x48, 0x8b, 0x44, 0x24, 0x28]); // mov rax, [rsp+28h]
        self.bytes(&[0xa8, 0x01]);
        let concat_cb = self.jump(Some(0x84));
        self.bytes(&[0x48, 0x8b, 0x48, 0x07]);
        self.bytes(&[0x48, 0x8d, 0x70, 0x0f]);
        let concat_db = self.jump(None);
        self.here(concat_cb);
        self.bytes(&[0x48, 0x8b, 0x08]);
        self.bytes(&[0x48, 0x8d, 0x70, 0x08]);
        self.here(concat_db);
        self.bytes(&[0xf3, 0xa4]); // rep movsb
        self.bytes(&[0x48, 0x8d, 0x43, 0x01]); // lea rax, [rbx+1]
        self.bytes(&[0x48, 0x83, 0xc4, 0x40]);
        self.bytes(&[0x5f, 0x5e, 0x5b]); // restore rdi, rsi, rbx
        self.bytes(&[0xc3]);
        self.rt_strlen = self.code.len();
        // rcx = value -> rax = byte length.
        self.bytes(&[0x48, 0x89, 0xc8]); // mov rax, rcx
        self.bytes(&[0xa8, 0x01]); // test al, 1
        let strlen_i = self.jump(Some(0x84));
        self.bytes(&[0x48, 0x8b, 0x41, 0x07]); // mov rax, [rcx+7]
        let strlen_d = self.jump(None);
        self.here(strlen_i);
        self.bytes(&[0x48, 0x8b, 0x01]); // mov rax, [rcx]
        self.here(strlen_d);
        self.bytes(&[0xc3]);
        self.rt_sbyte = self.code.len();
        // rcx = value, rdx = index -> rax = byte; out of range exits 72.
        self.bytes(&[0x53]); // push rbx
        self.bytes(&[0x48, 0x83, 0xec, 0x20]);
        self.bytes(&[0x48, 0x89, 0xcb]); // mov rbx, rcx
        self.bytes(&[0xe8]);
        let sbyte_len = self.hole();
        self.patch(sbyte_len, self.rt_strlen);
        self.bytes(&[0x48, 0x39, 0xc2]); // cmp rdx, rax
        let sbyte_ok = self.jump(Some(0x82)); // jb
        self.bytes(&[0xb9]);
        self.dword(72);
        self.bytes(&[0x41, 0xff, 0x57, 0x10]); // call ExitProcess
        self.here(sbyte_ok);
        self.bytes(&[0xf6, 0xc3, 0x01]); // test bl, 1
        let sbyte_i = self.jump(Some(0x84));
        self.bytes(&[0x8a, 0x44, 0x13, 0x0f]); // mov al, [rbx+rdx+15]
        let sbyte_d = self.jump(None);
        self.here(sbyte_i);
        self.bytes(&[0x8a, 0x44, 0x13, 0x08]); // mov al, [rbx+rdx+8]
        self.here(sbyte_d);
        self.bytes(&[0x48, 0x0f, 0xb6, 0xc0]); // movzx rax, al
        self.bytes(&[0x48, 0x83, 0xc4, 0x20]);
        self.bytes(&[0x5b]); // pop rbx
        self.bytes(&[0xc3]);
        // Dynamic arrays: [rc][len][capacity][elements..]. Unique arrays
        // reuse capacity; shared arrays detach before mutation.
        self.rt_dnew = self.code.len();
        // count and capacity survive the allocator in saved registers.
        self.bytes(&[0x53, 0x41, 0x54, 0x48, 0x83, 0xec, 0x28]);
        self.bytes(&[0x48, 0x89, 0xcb, 0x48, 0x81, 0xf9]);
        self.dword(MAX_DYN_ELEMENTS);
        let dnew_size_ok = self.jump(Some(0x86));
        self.bytes(&[0xb9]);
        self.dword(73);
        self.bytes(&[0x41, 0xff, 0x57, 0x10]);
        self.here(dnew_size_ok);
        self.bytes(&[0x41, 0xbc, 1, 0, 0, 0]); // capacity = 1
        let capacity = self.code.len();
        self.bytes(&[0x49, 0x39, 0xdc]); // cmp r12, rbx
        let capacity_done = self.jump(Some(0x83));
        self.bytes(&[0x49, 0xd1, 0xe4]); // capacity *= 2
        let capacity_again = self.jump(None);
        self.patch(capacity_again, capacity);
        self.here(capacity_done);
        self.bytes(&[0x4c, 0x89, 0xe1, 0x48, 0xc1, 0xe1, 3, 0x48, 0x83, 0xc1, 24]);
        self.bytes(&[0xe8]);
        self.relative(self.rt_alloc);
        self.bytes(&[0x48, 0xc7, 0x00, 1, 0, 0, 0]);
        self.bytes(&[0x48, 0x89, 0x58, 8, 0x4c, 0x89, 0x60, 16, 0x48, 0xff, 0xc0]);
        self.bytes(&[0x48, 0x83, 0xc4, 0x28, 0x41, 0x5c, 0x5b, 0xc3]);
        self.rt_dlen = self.code.len();
        self.bytes(&[0x48, 0x8b, 0x41, 7, 0xc3]);
        self.rt_didx = self.code.len();
        // Leaf load; error path reserves shadow space and aligns the stack.
        self.bytes(&[0x48, 0x3b, 0x51, 7]);
        let didx_ok = self.jump(Some(0x82));
        self.bytes(&[0x48, 0x83, 0xec, 0x28, 0xb9]);
        self.dword(72);
        self.bytes(&[0x41, 0xff, 0x57, 0x10]);
        self.here(didx_ok);
        self.bytes(&[0x48, 0x8b, 0x44, 0xd1, DYN_DATA, 0xc3]);
        self.rt_dfree_str = self.code.len();
        // rcx = untagged block. Saved registers survive nested releases.
        self.bytes(&[0x53, 0x41, 0x54, 0x48, 0x83, 0xec, 0x28]);
        self.bytes(&[0x48, 0x89, 0xcb, 0x4c, 0x8b, 0x63, 8]);
        let dfree_loop = self.code.len();
        self.bytes(&[0x4d, 0x85, 0xe4]);
        let dfree_end = self.jump(Some(0x84));
        self.bytes(&[0x4a, 0x8b, 0x4c, 0xe3, 16, 0xe8]);
        self.relative(self.rt_release);
        self.bytes(&[0x49, 0xff, 0xcc]); // dec r12 (not r12's 32-bit half)
        let dfree_again = self.jump(None);
        self.patch(dfree_again, dfree_loop);
        self.here(dfree_end);
        self.bytes(&[0x41, 0xff, 0x57, 0x18, 0x48, 0x89, 0xc1]);
        self.bytes(&[0x31, 0xd2, 0x49, 0x89, 0xd8, 0x41, 0xff, 0x57, 0x28]);
        self.bytes(&[0x48, 0x83, 0xc4, 0x28, 0x41, 0x5c, 0x5b, 0xc3]);
        self.rt_drelease_str = self.code.len();
        self.bytes(&[0x48, 0xff, 0x49, 0xff]); // dec [tag-1]
        let drelease_live = self.jump(Some(0x85));
        self.bytes(&[0x48, 0xff, 0xc9]); // untag
        self.bytes(&[0xe9]); // tail call keeps the caller's aligned call frame
        self.relative(self.rt_dfree_str);
        self.here(drelease_live);
        self.bytes(&[0xc3]);
        self.rt_push_plain = self.dynamic_push(false);
        self.rt_push_str = self.dynamic_push(true);
        self.rt_dset_plain = self.dynamic_set(false);
        self.rt_dset_str = self.dynamic_set(true);
        self.printd = self.code.len();
        // Fixed-point decimal print for f64: "%f" with six fraction digits,
        // plus nan/inf spellings. RCX carries the bit pattern, like printi.
        self.bytes(&[0x53]); // push rbx
        self.bytes(&[0x48, 0x83, 0xec, 0x60]); // sub rsp, 0x60
        self.bytes(&[0x48, 0x89, 0xcb]); // mov rbx, rcx
        self.bytes(&[0x66, 0x48, 0x0f, 0x6e, 0xc1]); // movq xmm0, rcx
        self.bytes(&[0x66, 0x0f, 0x2e, 0xc0]); // ucomisd xmm0, xmm0
        let finite = self.jump(Some(0x8b)); // jnp: ordered
        // NaN.
        self.bytes(&[0xb9]);
        self.dword(0xfffffff5); // mov ecx, -11
        self.bytes(&[0x41, 0xff, 0x17]); // call [r15]
        self.bytes(&[0x48, 0x89, 0xc1]); // mov rcx, rax
        self.data_ref(&[0x48, 0x8d, 0x15], DATA_NAN); // lea rdx, [rip+nan]
        self.bytes(&[0x41, 0xb8, 4, 0, 0, 0]); // mov r8d, 4
        self.bytes(&[0x4c, 0x8d, 0x4c, 0x24, 0x38]);
        self.bytes(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
        self.bytes(&[0x41, 0xff, 0x57, 0x08]); // call WriteFile
        self.bytes(&[0x48, 0x83, 0xc4, 0x60]);
        self.bytes(&[0x5b, 0xc3]);
        self.here(finite);
        self.bytes(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]); // minus = 0
        self.bytes(&[0x48, 0x85, 0xdb]); // test rbx, rbx
        let positive = self.jump(Some(0x89)); // jns
        self.bytes(&[0x48, 0xc7, 0x44, 0x24, 0x28, 1, 0, 0, 0]); // minus = 1
        self.bytes(&[0x48, 0xb8]);
        self.bytes(&0x7fffffffffffffffu64.to_le_bytes()); // mov rax, sign mask
        self.bytes(&[0x48, 0x21, 0xc3]); // and rbx, rax
        self.bytes(&[0x66, 0x48, 0x0f, 0x6e, 0xc3]); // movq xmm0, rbx
        self.here(positive);
        // |v| >= 2^63 does not fit the fixed-point integer part: print inf.
        self.data_ref(&[0xf2, 0x0f, 0x10, 0x0d], DATA_TWO_POW_63);
        self.bytes(&[0x66, 0x0f, 0x2e, 0xc1]); // ucomisd xmm0, xmm1
        let not_inf = self.jump(Some(0x82)); // jb
        self.bytes(&[0xb9]);
        self.dword(0xfffffff5);
        self.bytes(&[0x41, 0xff, 0x17]);
        self.bytes(&[0x48, 0x89, 0xc1]);
        self.bytes(&[0x48, 0x83, 0x7c, 0x24, 0x28, 0x00]); // cmp minus, 0
        self.data_ref(&[0x48, 0x8d, 0x15], DATA_INF);
        self.bytes(&[0x41, 0xb8, 4, 0, 0, 0]); // mov r8d, 4
        let minus_inf = self.jump(Some(0x84)); // je
        self.data_ref(&[0x48, 0x8d, 0x15], DATA_NEG_INF);
        self.bytes(&[0x41, 0xb8, 5, 0, 0, 0]); // mov r8d, 5
        self.here(minus_inf);
        self.bytes(&[0x4c, 0x8d, 0x4c, 0x24, 0x38]);
        self.bytes(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
        self.bytes(&[0x41, 0xff, 0x57, 0x08]);
        self.bytes(&[0x48, 0x83, 0xc4, 0x60]);
        self.bytes(&[0x5b, 0xc3]);
        self.here(not_inf);
        // Split into integer and rounded fraction; no calls before this point,
        // so xmm0 survives.
        self.bytes(&[0xf2, 0x48, 0x0f, 0x2c, 0xc0]); // cvttsd2si rax, xmm0
        self.bytes(&[0x48, 0x89, 0x44, 0x24, 0x40]); // mov [rsp+40h], rax
        self.bytes(&[0xf2, 0x48, 0x0f, 0x2a, 0xc8]); // cvtsi2sd xmm1, rax
        self.bytes(&[0xf2, 0x0f, 0x5c, 0xc1]); // subsd xmm0, xmm1
        self.data_ref(&[0xf2, 0x0f, 0x59, 0x05], DATA_MILLION); // mulsd xmm0, 1e6
        self.data_ref(&[0xf2, 0x0f, 0x58, 0x05], DATA_HALF); // addsd xmm0, 0.5
        self.bytes(&[0xf2, 0x48, 0x0f, 0x2c, 0xd0]); // cvttsd2si rdx, xmm0
        self.bytes(&[0x48, 0x89, 0x54, 0x24, 0x38]); // mov [rsp+38h], rdx
        // The rounded fraction may reach 1000000: carry into the integer part.
        self.bytes(&[0x48, 0x81, 0xfa]);
        self.dword(1000000); // cmp rdx, 1000000
        let no_carry = self.jump(Some(0x82)); // jb
        self.bytes(&[0x48, 0x81, 0xea]);
        self.dword(1000000); // sub rdx, 1000000
        self.bytes(&[0x48, 0x89, 0x54, 0x24, 0x38]); // mov [rsp+38h], rdx
        self.bytes(&[0x48, 0xff, 0x44, 0x24, 0x40]); // inc qword [rsp+40h]
        self.here(no_carry);
        // Handle, buffer end and newline; digits are built backwards from it.
        self.bytes(&[0xb9]);
        self.dword(0xfffffff5);
        self.bytes(&[0x41, 0xff, 0x17]);
        self.bytes(&[0x48, 0x89, 0x44, 0x24, 0x30]); // mov [rsp+30h], rax
        self.bytes(&[0x4c, 0x8d, 0x54, 0x24, 0x60]); // lea r10, [rsp+60h]
        self.bytes(&[0x49, 0xff, 0xca]); // dec r10
        self.bytes(&[0x41, 0xc6, 0x02, 0x0a]); // mov byte [r10], 10
        self.bytes(&[0x41, 0xb9, 6, 0, 0, 0]); // mov r9d, 6
        self.bytes(&[0xb9, 0x0a, 0, 0, 0]); // mov ecx, 10
        let frac = self.code.len();
        self.bytes(&[0x48, 0x8b, 0x44, 0x24, 0x38]); // mov rax, [rsp+38h]
        self.bytes(&[0x31, 0xd2]); // xor edx, edx
        self.bytes(&[0x48, 0xf7, 0xf1]); // div rcx
        self.bytes(&[0x48, 0x89, 0x44, 0x24, 0x38]); // mov [rsp+38h], rax
        self.bytes(&[0x80, 0xc2, 0x30]); // add dl, '0'
        self.bytes(&[0x49, 0xff, 0xca]); // dec r10
        self.bytes(&[0x41, 0x88, 0x12]); // mov [r10], dl
        self.bytes(&[0x41, 0xff, 0xc9]); // dec r9d
        let frac_again = self.jump(Some(0x85));
        self.patch(frac_again, frac);
        self.bytes(&[0x49, 0xff, 0xca]); // dec r10
        self.bytes(&[0x41, 0xc6, 0x02, 0x2e]); // mov byte [r10], '.'
        self.bytes(&[0x48, 0x8b, 0x44, 0x24, 0x40]); // mov rax, [rsp+40h]
        let int_loop = self.code.len();
        self.bytes(&[0x31, 0xd2]); // xor edx, edx
        self.bytes(&[0x48, 0xf7, 0xf1]); // div rcx
        self.bytes(&[0x80, 0xc2, 0x30]); // add dl, '0'
        self.bytes(&[0x49, 0xff, 0xca]); // dec r10
        self.bytes(&[0x41, 0x88, 0x12]); // mov [r10], dl
        self.bytes(&[0x48, 0x85, 0xc0]); // test rax, rax
        let int_again = self.jump(Some(0x85));
        self.patch(int_again, int_loop);
        self.bytes(&[0x48, 0x83, 0x7c, 0x24, 0x28, 0x00]); // cmp minus, 0
        let write = self.jump(Some(0x84)); // je
        self.bytes(&[0x49, 0xff, 0xca]); // dec r10
        self.bytes(&[0x41, 0xc6, 0x02, 0x2d]); // mov byte [r10], '-'
        self.here(write);
        self.bytes(&[0x48, 0x8b, 0x4c, 0x24, 0x30]); // mov rcx, [rsp+30h]
        self.bytes(&[0x4c, 0x89, 0xd2]); // mov rdx, r10
        self.bytes(&[0x4c, 0x8d, 0x44, 0x24, 0x60]); // lea r8, [rsp+60h]
        self.bytes(&[0x4d, 0x29, 0xd0]); // sub r8, r10
        self.bytes(&[0x4c, 0x8d, 0x4c, 0x24, 0x38]); // lea r9, [rsp+38h]
        self.bytes(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
        self.bytes(&[0x41, 0xff, 0x57, 0x08]); // call WriteFile
        self.bytes(&[0x48, 0x83, 0xc4, 0x60]);
        self.bytes(&[0x5b, 0xc3]);
        main
    }
}
// Fold only fully constant, side-effect-free trees. Invalid division stays in
// generated code and follows the same runtime exit rules as variable division.
// Narrow results are canonicalized to match the emitted instructions.
fn canonical_value(ty: &Type, value: u64) -> u64 {
    match ty {
        Type::I32 => ((value as u32) as i32) as u64,
        Type::U32 => value & 0xffff_ffff,
        Type::I16 => ((value as u16) as i16) as u64,
        Type::U16 => value & 0xffff,
        Type::I8 => ((value as u8) as i8) as u64,
        Type::U8 => value & 0xff,
        _ => value,
    }
}

fn constant(expr: &Expr) -> Option<u64> {
    let narrow = expr.ty.narrow();
    // Narrow logical shifts move the width value, so the operand is masked to
    // the width first: a 64-bit shift would leak sign-extension bits downward.
    let width_mask = if narrow {
        (1u64 << expr.ty.bits()) - 1
    } else {
        u64::MAX
    };
    let shift_mask = match expr.ty.bits() {
        8 => 7u64,
        16 => 15,
        32 => 31,
        _ => 63,
    };
    // Reinterpret a canonical value as the signed type of the expression.
    let signed = |value: u64| match expr.ty {
        Type::I8 => (value as u8) as i8 as i64,
        Type::I16 => (value as u16) as i16 as i64,
        Type::I32 => (value as u32) as i32 as i64,
        _ => value as i64,
    };
    match &expr.kind {
        ExprKind::Number(n) => Some(canonical_value(&expr.ty, *n)),
        ExprKind::Float(value) => Some(if expr.ty.single() {
            (*value as f32).to_bits() as u64
        } else {
            value.to_bits()
        }),
        ExprKind::Bool(value) => Some(u64::from(*value)),
        ExprKind::Cast(ty, value) => constant(value).map(|n| {
            if value.ty.float() {
                let (x, single) = if value.ty.single() {
                    (f64::from(f32::from_bits(n as u32)), true)
                } else {
                    (f64::from_bits(n), false)
                };
                if *ty == Type::Bool {
                    return u64::from(!x.is_nan() && x != 0.0);
                }
                if *ty == Type::F64 {
                    return if single { x.to_bits() } else { n };
                }
                if *ty == Type::F32 {
                    return (x as f32).to_bits() as u64;
                }
                if ty.unsigned() && !x.is_nan() && x >= 9223372036854775808.0 {
                    // Same split the emitted code performs: the shifted value
                    // converts with cvtt rules (out of range is the
                    // integer-indefinite pattern), then 2^63 adds back.
                    let shifted = if single {
                        (f32::from_bits(n as u32) - 9223372036854775808.0f32) as f64
                    } else {
                        x - 9223372036854775808.0
                    };
                    let low = if !(-9223372036854775808.0..9223372036854775808.0).contains(&shifted)
                    {
                        0x8000000000000000u64
                    } else {
                        (shifted as i64) as u64
                    };
                    return canonical_value(ty, low.wrapping_add(0x8000000000000000));
                }
                // cvttsd2si/cvttss2si emulation: out of range or NaN yields
                // the hardware integer-indefinite pattern.
                let raw = if x.is_nan()
                    || !(-9223372036854775808.0..9223372036854775808.0).contains(&x)
                {
                    0x8000000000000000u64
                } else {
                    (x as i64) as u64
                };
                return canonical_value(ty, raw);
            }
            if *ty == Type::F32 {
                if value.ty == Type::U64 && n >= (1u64 << 63) {
                    // Same split the emitted code performs.
                    return (((n >> 1) as i64 as f32 * 2.0 + (n & 1) as i64 as f32).to_bits())
                        as u64;
                }
                return ((n as i64 as f32).to_bits()) as u64;
            }
            if *ty == Type::F64 {
                if value.ty == Type::U64 && n >= (1u64 << 63) {
                    // Same split the emitted code performs.
                    return ((n >> 1) as i64 as f64 * 2.0 + (n & 1) as i64 as f64).to_bits();
                }
                return (n as i64 as f64).to_bits();
            }
            if *ty == Type::Bool {
                u64::from(n != 0)
            } else {
                canonical_value(ty, n)
            }
        }),
        ExprKind::Unary(op, e) => {
            let value = constant(e)?;
            if expr.ty.float() {
                if expr.ty.single() {
                    let x = f32::from_bits(value as u32);
                    return match *op {
                        // Sign-bit flip matches the emitted xorps, including -0.0.
                        "-" => Some(((value as u32) ^ 0x80000000) as u64),
                        "!" => Some(u64::from(!x.is_nan() && x != 0.0)),
                        _ => None,
                    };
                }
                let x = f64::from_bits(value);
                return match *op {
                    // Sign-bit flip matches the emitted xorpd, including -0.0.
                    "-" => Some(value ^ 0x8000000000000000),
                    "!" => Some(u64::from(!x.is_nan() && x != 0.0)),
                    _ => None,
                };
            }
            Some(match *op {
                "+" => value,
                "-" => canonical_value(&expr.ty, value.wrapping_neg()),
                "!" => u64::from(value == 0),
                "~" => canonical_value(&expr.ty, !value),
                _ => return None,
            })
        }
        ExprKind::Binary(op, left, right) => {
            let a = constant(left)?;
            let b = constant(right)?;
            if left.ty.single() {
                // Rust f32 arithmetic compiles to the same SSE instructions.
                let x = f32::from_bits(a as u32);
                let y = f32::from_bits(b as u32);
                return Some(match *op {
                    "+" => ((x + y).to_bits()) as u64,
                    "-" => ((x - y).to_bits()) as u64,
                    "*" => ((x * y).to_bits()) as u64,
                    "/" => ((x / y).to_bits()) as u64,
                    "==" => u64::from(x == y),
                    "!=" => u64::from(x != y),
                    "<" => u64::from(x < y),
                    ">" => u64::from(x > y),
                    "<=" => u64::from(x <= y),
                    ">=" => u64::from(x >= y),
                    _ => return None,
                });
            }
            if left.ty.float() {
                // Rust f64 arithmetic compiles to the same SSE instructions.
                let x = f64::from_bits(a);
                let y = f64::from_bits(b);
                return Some(match *op {
                    "+" => (x + y).to_bits(),
                    "-" => (x - y).to_bits(),
                    "*" => (x * y).to_bits(),
                    "/" => (x / y).to_bits(),
                    "==" => u64::from(x == y),
                    "!=" => u64::from(x != y),
                    "<" => u64::from(x < y),
                    ">" => u64::from(x > y),
                    "<=" => u64::from(x <= y),
                    ">=" => u64::from(x >= y),
                    _ => return None,
                });
            }
            let canonical = |value: u64| canonical_value(&expr.ty, value);
            Some(match *op {
                "+" => canonical(a.wrapping_add(b)),
                "-" => canonical(a.wrapping_sub(b)),
                "*" => canonical(a.wrapping_mul(b)),
                "&" => a & b,
                "|" => a | b,
                "^" => a ^ b,
                "<<" => canonical(a.wrapping_shl((b & shift_mask) as u32)),
                ">>" if left.ty.unsigned() => a.wrapping_shr((b & shift_mask) as u32),
                ">>" if narrow => canonical(signed(a).wrapping_shr((b & shift_mask) as u32) as u64),
                ">>" => (a as i64).wrapping_shr((b & shift_mask) as u32) as u64,
                // The width value shifts logically, then re-enters canonical form.
                ">>>" if left.ty.unsigned() => a.wrapping_shr((b & shift_mask) as u32),
                ">>>" if narrow => canonical((a & width_mask) >> (b & shift_mask)),
                ">>>" => a.wrapping_shr((b & shift_mask) as u32),
                "/" if left.ty.unsigned() => a.checked_div(b)?,
                "%" if left.ty.unsigned() => a.checked_rem(b)?,
                // Division runs in the exact width so MIN / -1 refuses to fold
                // and keeps the runtime exit 71.
                "/" if narrow => canonical(match expr.ty {
                    Type::I8 => (a as u8 as i8).checked_div(b as u8 as i8)? as i64 as u64,
                    Type::I16 => (a as u16 as i16).checked_div(b as u16 as i16)? as i64 as u64,
                    _ => (a as u32 as i32).checked_div(b as u32 as i32)? as i64 as u64,
                }),
                "%" if narrow => canonical(match expr.ty {
                    Type::I8 => (a as u8 as i8).checked_rem(b as u8 as i8)? as i64 as u64,
                    Type::I16 => (a as u16 as i16).checked_rem(b as u16 as i16)? as i64 as u64,
                    _ => (a as u32 as i32).checked_rem(b as u32 as i32)? as i64 as u64,
                }),
                "/" => (a as i64).checked_div(b as i64)? as u64,
                "%" => (a as i64).checked_rem(b as i64)? as u64,
                "==" => u64::from(a == b),
                "!=" => u64::from(a != b),
                "<" if left.ty.unsigned() => u64::from(a < b),
                ">" if left.ty.unsigned() => u64::from(a > b),
                "<=" if left.ty.unsigned() => u64::from(a <= b),
                ">=" if left.ty.unsigned() => u64::from(a >= b),
                "<" => u64::from((a as i64) < (b as i64)),
                ">" => u64::from((a as i64) > (b as i64)),
                "<=" => u64::from((a as i64) <= (b as i64)),
                ">=" => u64::from((a as i64) >= (b as i64)),
                "&&" => u64::from(a != 0 && b != 0),
                "||" => u64::from(a != 0 || b != 0),
                _ => return None,
            })
        }
        _ => None,
    }
}

fn patch_relative(code: &mut [u8], at: usize, target: usize) {
    code[at..at + 4].copy_from_slice(&((target as i64 - at as i64 - 4) as i32).to_le_bytes());
}

fn emit_pe(code: &[u8], data: &[u8]) -> Vec<u8> {
    fn word(b: &mut [u8], at: usize, v: u16) {
        b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn dword(b: &mut [u8], at: usize, v: u32) {
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn qword(b: &mut [u8], at: usize, v: u64) {
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    let data_raw_size = data.len().next_multiple_of(0x200);
    let reloc_raw = 0x10800 + data_raw_size;
    let reloc_rva = (RDATA + data.len() as u32).next_multiple_of(0x1000);
    let mut b = vec![0; reloc_raw + 0x200];
    b[..2].copy_from_slice(b"MZ");
    dword(&mut b, 0x3c, 0x80);
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    word(&mut b, 0x84, 0x8664);
    word(&mut b, 0x86, 4);
    word(&mut b, 0x94, 0xf0);
    word(&mut b, 0x96, 0x22); // Executable, large-address aware; relocations retained.
    let o = 0x98;
    word(&mut b, o, 0x20b);
    dword(&mut b, o + 4, 0x10000);
    dword(&mut b, o + 8, (0x600 + data_raw_size) as u32);
    dword(&mut b, o + 0x10, TEXT);
    dword(&mut b, o + 0x14, TEXT);
    qword(&mut b, o + 0x18, 0x140000000);
    dword(&mut b, o + 0x20, 0x1000);
    dword(&mut b, o + 0x24, 0x200);
    word(&mut b, o + 0x28, 6);
    word(&mut b, o + 0x30, 6);
    dword(&mut b, o + 0x38, reloc_rva + 0x1000);
    dword(&mut b, o + 0x3c, 0x400);
    word(&mut b, o + 0x44, 3);
    word(&mut b, o + 0x46, 0x160); // NX, DynamicBase, HighEntropyVA.
    for (at, v) in [
        (0x48, 0x100000),
        (0x50, 0x1000),
        (0x58, 0x100000),
        (0x60, 0x1000),
    ] {
        qword(&mut b, o + at, v);
    }
    dword(&mut b, o + 0x6c, 16);
    dword(&mut b, o + 0x78, IMPORTS);
    dword(&mut b, o + 0x7c, 40);
    dword(&mut b, o + 0x98, reloc_rva);
    dword(&mut b, o + 0x9c, 12);
    dword(&mut b, o + 0xd0, IMPORTS + 0x40);
    dword(&mut b, o + 0xd4, 32);
    for (i, name, rva, raw, size, flags) in [
        (0, b".text\0\0\0", TEXT, 0x400, 0x10000, 0x60000020),
        (1, b".idata\0\0", IMPORTS, 0x10400, 0x400, 0xc0000040),
        (
            2,
            b".rdata\0\0",
            RDATA,
            0x10800,
            data_raw_size as u32,
            0x40000040,
        ),
        (
            3,
            b".reloc\0\0",
            reloc_rva,
            reloc_raw as u32,
            0x200,
            0x42000040,
        ),
    ] {
        let at = 0x188 + i * 40;
        b[at..at + 8].copy_from_slice(name);
        dword(
            &mut b,
            at + 8,
            match i {
                2 => data.len() as u32,
                3 => 12,
                _ => size,
            },
        );
        dword(&mut b, at + 12, rva);
        dword(&mut b, at + 16, size);
        dword(&mut b, at + 20, raw);
        dword(&mut b, at + 36, flags);
    }
    b[0x400..0x400 + code.len()].copy_from_slice(code);
    b[0x10800..0x10800 + data.len()].copy_from_slice(data);
    // Code/data references are RIP-relative or RVAs, so there are no absolute
    // addresses to fix. The IAT holds unbound RVAs that the loader resolves
    // directly: relocating those entries under forced rebase would corrupt
    // them, so the directory stays a valid ABSOLUTE padding block.
    dword(&mut b, reloc_raw, TEXT);
    dword(&mut b, reloc_raw + 4, 12);
    let i = 0x10400;
    dword(&mut b, i, IMPORTS + 0x40);
    dword(&mut b, i + 12, IMPORTS + 0x80);
    dword(&mut b, i + 16, IMPORTS + 0x40);
    // IAT order is fixed by the runtime: r15+0x00..0x28.
    for (at, rva) in [
        (0x40, 0x90),
        (0x48, 0xa0),
        (0x50, 0xb0),
        (0x58, 0xc0),
        (0x60, 0xd0),
        (0x68, 0xe0),
    ] {
        qword(&mut b, i + at, (IMPORTS + rva) as u64);
    }
    for (at, text) in [
        (0x80, &b"KERNEL32.DLL\0"[..]),
        (0x92, &b"GetStdHandle\0"[..]),
        (0xa2, &b"WriteFile\0"[..]),
        (0xb2, &b"ExitProcess\0"[..]),
        (0xc2, &b"GetProcessHeap\0"[..]),
        (0xd2, &b"HeapAlloc\0"[..]),
        (0xe2, &b"HeapFree\0"[..]),
    ] {
        b[i + at..i + at + text.len()].copy_from_slice(text);
    }
    b
}
