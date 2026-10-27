//! Small, deterministic x86-64 backend. Every write stays inside an owned Vec.
use crate::{
    Diagnostic,
    lexer::Keyword,
    parser::{Expr, ExprKind, Program, Stmt, StmtKind, Type},
};
use std::{collections::HashMap, ops::Range};

const TEXT: u32 = 0x1000;
const IMPORTS: u32 = 0x11000;
const MAX_CODE: usize = 0x10000;
const RDATA: u32 = 0x12000;
const MAX_DATA: usize = 2 * 1024 * 1024;

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
        data: vec![0; 8], // Static empty-string descriptor.
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
            g.bytes(
                &[
                    [0x48, 0x89, 0x8d],
                    [0x48, 0x89, 0x95],
                    [0x4c, 0x89, 0x85],
                    [0x4c, 0x89, 0x8d],
                ][i],
            );
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
            }
        }
        g.block(&function.body)?;
        // Win64 calls require 16-byte stack alignment. Slots from disjoint
        // lexical scopes share space; reserve only the maximum live count.
        let frame_size = (g.peak_slots * 8).next_multiple_of(16) as u32;
        g.code[frame..frame + 4].copy_from_slice(&frame_size.to_le_bytes());
        if g.aggregate_result {
            g.bytes(&[0x0f, 0x0b]); // Definite-return checking makes this unreachable.
        } else if function.result == Type::Str {
            g.string_literal("")?;
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
}
struct Generator<'a> {
    source: &'a str,
    code: Vec<u8>,
    functions: HashMap<String, (usize, usize)>,
    offsets: Vec<usize>,
    calls: Vec<(usize, usize)>,
    vars: Vec<(String, usize)>,
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
        if self.vars.iter().any(|(n, _)| n == name) {
            return Err(self.error(span, format!("variable already defined: {name}")));
        }
        if self.slots + ty.slots() > 256 + usize::from(self.aggregate_result) {
            return Err(self.error(span, "local variable limit exceeded"));
        }
        let slot = self.slots;
        self.slots += ty.slots();
        self.peak_slots = self.peak_slots.max(self.slots);
        self.vars.push((name.to_owned(), slot));
        Ok(slot)
    }
    fn variable(&self, name: &str, span: &Range<usize>) -> Result<usize, Diagnostic> {
        self.vars
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, s)| *s)
            .ok_or_else(|| self.error(span, format!("undefined variable: {name}")))
    }
    fn local(&mut self, opcode: u8, slot: usize) {
        self.bytes(&[0x48, opcode, 0x85]);
        self.dword((-(8 * (slot as i32 + 1))) as u32);
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
                for index in 1..*count {
                    for slot in 0..value.ty.slots() {
                        self.bytes(&[0x48, 0x8b, 0x84, 0x24]);
                        self.dword(((self.stack - end + offset + slot) * 8) as u32);
                        self.snapshot_store(end, offset + index * value.ty.slots() + slot);
                    }
                }
            }
            _ => {
                let initial_stack = self.stack;
                self.expr(value)?;
                if value.ty.aggregate() {
                    self.bytes(&[0x48, 0x89, 0xc1]);
                    for index in 0..value.ty.slots() {
                        self.bytes(&[0x48, 0x8b, 0x81]);
                        self.dword((index * 8) as u32);
                        self.snapshot_store(end, offset + index);
                    }
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
    fn store_aggregate(&mut self, slot: usize, value: &Expr) -> Result<(), Diagnostic> {
        self.snapshot(value)?;
        for index in 0..value.ty.slots() {
            self.pop(&[0x58]);
            self.local(0x89, slot + value.ty.slots() - 1 - index);
        }
        Ok(())
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
            self.expr(array)?;
            self.push();
            self.expr(index)?;
            self.pop(&[0x59]);
            self.bytes(&[0x48, 0x3b, 0x01]);
            let in_bounds = self.jump(Some(0x82));
            self.exit(72);
            self.here(in_bounds);
            self.bytes(&[0x48, 0x8d, 0x44, 0x01, 0x08]);
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
    fn statement(&mut self, stmt: &Stmt) -> Result<(), Diagnostic> {
        match &stmt.kind {
            StmtKind::Let(name, _, value) => {
                if value.ty.aggregate() {
                    let slot = self.declare(name, value.ty.clone(), &stmt.span)?;
                    return self.store_aggregate(slot, value);
                }
                self.expr(value)?;
                let slot = self.declare(name, value.ty.clone(), &stmt.span)?;
                self.local(0x89, slot);
            }
            StmtKind::Assign(name, value) => {
                let slot = self.variable(name, &stmt.span)?;
                if value.ty.aggregate() {
                    return self.store_aggregate(slot, value);
                }
                self.expr(value)?;
                self.local(0x89, slot);
            }
            StmtKind::IndexAssign(target, value) => {
                self.index_address(target)?;
                self.push();
                if value.ty.aggregate() {
                    self.snapshot(value)?;
                    self.bytes(&[0x48, 0x8b, 0x8c, 0x24]);
                    self.dword((value.ty.slots() * 8) as u32);
                    for index in 0..value.ty.slots() {
                        self.pop(&[0x58]);
                        self.bytes(&[0x48, 0x89, 0x81]);
                        self.dword((index * 8) as u32);
                    }
                    self.pop(&[0x5a]);
                } else {
                    self.expr(value)?;
                    self.pop(&[0x59]);
                    self.bytes(&[0x48, 0x89, 0x01]);
                }
            }
            StmtKind::Expr(expr) => {
                let initial_stack = self.stack;
                self.expr(expr)?;
                self.restore_stack(initial_stack);
            }
            StmtKind::Return(expr) => {
                if self.aggregate_result {
                    self.snapshot(expr)?;
                    self.local(0x8b, 0);
                    self.bytes(&[0x48, 0x89, 0xc1]);
                    for index in 0..expr.ty.slots() {
                        self.pop(&[0x58]);
                        self.bytes(&[0x48, 0x89, 0x81]);
                        self.dword((index * 8) as u32);
                    }
                } else {
                    self.expr(expr)?;
                }
                self.bytes(&[0xc9, 0xc3]);
            }
            StmtKind::Block(body) => self.block(body)?,
            StmtKind::Builtin(kind, expr) => {
                self.expr(expr)?;
                self.bytes(&[0x48, 0x89, 0xc1]);
                let amount = self.reserve_call();
                if *kind == Keyword::Exit {
                    self.bytes(&[0x41, 0xff, 0x57, 0x10]);
                } else {
                    self.bytes(&[0xe8]);
                    self.relative(if *kind == Keyword::Printi {
                        if expr.ty == Type::U64 {
                            self.printu
                        } else {
                            self.printi
                        }
                    } else if matches!(kind, Keyword::Print | Keyword::Println) {
                        self.printstr
                    } else {
                        self.printc
                    });
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
                self.bytes(&[0x48, 0x85, 0xc0]);
                let end = self.jump(Some(0x84));
                self.loops.push(Loop {
                    start,
                    breaks: Vec::new(),
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
                let at = self.jump(None);
                if let Some(loop_) = self.loops.last_mut() {
                    loop_.breaks.push(at);
                }
            }
            StmtKind::Continue => {
                let Some(loop_) = self.loops.last() else {
                    return Err(self.error(&stmt.span, "continue is only valid inside a loop"));
                };
                let target = loop_.start;
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
            return Ok(());
        }
        match &expr.kind {
            ExprKind::String(value) => self.string_literal(value)?,
            ExprKind::Number(n) => {
                self.bytes(&[0x48, 0xb8]);
                self.bytes(&n.to_le_bytes());
            }
            ExprKind::Array(_) | ExprKind::Repeat(..) | ExprKind::Record(..) => {
                return Err(self.error(&expr.span, "aggregate literal requires local storage"));
            }
            ExprKind::Field(..) => {
                self.index_address(expr)?;
                if !expr.ty.aggregate() {
                    self.bytes(&[0x48, 0x8b, 0x00]);
                }
            }
            ExprKind::Index(base, _) => {
                self.index_address(expr)?;
                if base.ty == Type::Str {
                    self.bytes(&[0x0f, 0xb6, 0x00]);
                } else if !expr.ty.aggregate() {
                    self.bytes(&[0x48, 0x8b, 0x00]);
                }
            }
            ExprKind::Len(value) => {
                if value.ty == Type::Str {
                    self.expr(value)?;
                    self.bytes(&[0x48, 0x8b, 0x00]);
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
                if *ty == Type::Bool {
                    self.bytes(&[0x48, 0x85, 0xc0, 0x0f, 0x95, 0xc0, 0x48, 0x0f, 0xb6, 0xc0]);
                }
            }
            ExprKind::Var(name) => {
                let slot = self.variable(name, &expr.span)?;
                if expr.ty.aggregate() {
                    self.local(0x8d, slot + expr.ty.slots() - 1);
                } else {
                    self.local(0x8b, slot);
                }
            }
            ExprKind::Unary(op, value) => {
                self.expr(value)?;
                match *op {
                    "-" => self.bytes(&[0x48, 0xf7, 0xd8]),
                    "~" => self.bytes(&[0x48, 0xf7, 0xd0]),
                    "!" => {
                        self.bytes(&[0x48, 0x85, 0xc0, 0x0f, 0x94, 0xc0, 0x48, 0x0f, 0xb6, 0xc0])
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
                for arg in args {
                    if arg.ty.aggregate() {
                        self.snapshot(arg)?;
                        self.bytes(&[0x48, 0x89, 0xe0]);
                    } else {
                        self.expr(arg)?;
                    }
                    self.push();
                    locations.push(self.stack);
                }
                if args.len() > 4 || aggregates || expr.ty.aggregate() {
                    // Preserve evaluated arguments above a separate outgoing
                    // call area. Nested calls cannot overwrite these values.
                    let base = 32 + args.len().saturating_sub(4) * 8;
                    let amount = base + ((self.stack * 8 + base) % 16);
                    self.bytes(&[0x48, 0x81, 0xec]);
                    self.dword(amount as u32);
                    for (arg, location) in locations.iter().enumerate() {
                        let source = amount + (self.stack - location) * 8;
                        if arg < 4 {
                            self.bytes(
                                &[
                                    [0x48, 0x8b, 0x8c, 0x24],
                                    [0x48, 0x8b, 0x94, 0x24],
                                    [0x4c, 0x8b, 0x84, 0x24],
                                    [0x4c, 0x8b, 0x8c, 0x24],
                                ][arg],
                            );
                            self.dword(source as u32);
                        } else {
                            self.bytes(&[0x4c, 0x8b, 0x94, 0x24]);
                            self.dword(source as u32);
                            self.bytes(&[0x4c, 0x89, 0x94, 0x24]);
                            self.dword((32 + (arg - 4) * 8) as u32);
                        }
                    }
                    if expr.ty.aggregate() {
                        self.bytes(&[0x4c, 0x8d, 0x94, 0x24]);
                        self.dword((amount + (self.stack - result_stack) * 8) as u32);
                    }
                    self.bytes(&[0xe8]);
                    let at = self.hole();
                    self.calls.push((at, index));
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
                    self.pop([&[0x59][..], &[0x5a], &[0x41, 0x58], &[0x41, 0x59]][arg]);
                }
                let amount = self.reserve_call();
                self.bytes(&[0xe8]);
                let at = self.hole();
                self.calls.push((at, index));
                self.release_call(amount);
            }
            ExprKind::Binary(op, left, right) if *op == "&&" || *op == "||" => {
                self.expr(left)?;
                self.bytes(&[0x48, 0x85, 0xc0]);
                let skip = self.jump(Some(if *op == "&&" { 0x84 } else { 0x85 }));
                self.expr(right)?;
                self.bytes(&[0x48, 0x85, 0xc0]);
                self.here(skip);
                // Both paths arrive with flags from their own test instruction.
                self.bytes(&[0x0f, 0x95, 0xc0, 0x48, 0x0f, 0xb6, 0xc0]);
            }
            ExprKind::Binary(op, left, right) => {
                self.expr(left)?;
                self.push();
                self.expr(right)?;
                self.bytes(&[0x48, 0x89, 0xc1]);
                self.pop(&[0x58]);
                if left.ty == Type::Str {
                    self.bytes(&[0x48, 0x89, 0xca, 0x48, 0x89, 0xc1]);
                    let amount = self.reserve_call();
                    self.bytes(&[0xe8]);
                    self.relative(self.streq);
                    self.release_call(amount);
                    if *op == "!=" {
                        self.bytes(&[0x83, 0xf0, 1]);
                    }
                    return Ok(());
                }
                match *op {
                    "+" => self.bytes(&[0x48, 0x01, 0xc8]),
                    "-" => self.bytes(&[0x48, 0x29, 0xc8]),
                    "*" => self.bytes(&[0x48, 0x0f, 0xaf, 0xc1]),
                    "&" => self.bytes(&[0x48, 0x21, 0xc8]),
                    "|" => self.bytes(&[0x48, 0x09, 0xc8]),
                    "^" => self.bytes(&[0x48, 0x31, 0xc8]),
                    "<<" => self.bytes(&[0x48, 0xd3, 0xe0]),
                    ">>" => {
                        self.bytes(&[0x48, 0xd3, if left.ty == Type::U64 { 0xe8 } else { 0xf8 }])
                    }
                    ">>>" => self.bytes(&[0x48, 0xd3, 0xe8]),
                    "/" | "%" => {
                        self.bytes(&[0x48, 0x85, 0xc9]);
                        let nonzero = self.jump(Some(0x85));
                        self.exit(70);
                        self.here(nonzero);
                        if left.ty == Type::U64 {
                            self.bytes(&[0x31, 0xd2, 0x48, 0xf7, 0xf1]);
                            if *op == "%" {
                                self.bytes(&[0x48, 0x89, 0xd0]);
                            }
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
                                if left.ty == Type::U64 {
                                    0x92
                                } else {
                                    0x9c
                                }
                            }
                            ">" => {
                                if left.ty == Type::U64 {
                                    0x97
                                } else {
                                    0x9f
                                }
                            }
                            "<=" => {
                                if left.ty == Type::U64 {
                                    0x96
                                } else {
                                    0x9e
                                }
                            }
                            ">=" => {
                                if left.ty == Type::U64 {
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
        self.bytes(&[
            0x48, 0x83, 0xec, 0x48, 0x48, 0x89, 0x4c, 0x24, 0x30, 0xb9, 0xf5, 0xff, 0xff, 0xff,
            0x41, 0xff, 0x17, 0x48, 0x8b, 0x54, 0x24, 0x30, 0x4c, 0x8b, 0x02, 0x48, 0x83, 0xc2, 8,
            0x48, 0x89, 0xc1, 0x4c, 0x8d, 0x4c, 0x24, 0x38, 0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0,
            0, 0x41, 0xff, 0x57, 8, 0x48, 0x83, 0xc4, 0x48, 0xc3,
        ]);
        self.streq = self.code.len();
        self.bytes(&[0x4c, 0x8b, 0x01, 0x4c, 0x3b, 0x02]);
        let unequal_length = self.jump(Some(0x85));
        self.bytes(&[0x31, 0xc0]);
        let compare = self.code.len();
        self.bytes(&[0x4c, 0x39, 0xc0]);
        let equal = self.jump(Some(0x84));
        self.bytes(&[0x44, 0x8a, 0x4c, 0x01, 8, 0x44, 0x3a, 0x4c, 0x02, 8]);
        let unequal_byte = self.jump(Some(0x85));
        self.bytes(&[0x48, 0xff, 0xc0]);
        let next = self.jump(None);
        self.patch(next, compare);
        self.here(equal);
        self.bytes(&[0xb8, 1, 0, 0, 0, 0xc3]);
        self.here(unequal_length);
        self.here(unequal_byte);
        self.bytes(&[0x31, 0xc0, 0xc3]);
        main
    }
}
// Fold only fully constant, side-effect-free trees. Invalid division stays in
// generated code and follows the same runtime exit rules as variable division.
fn constant(expr: &Expr) -> Option<u64> {
    match &expr.kind {
        ExprKind::Number(n) => Some(*n),
        ExprKind::Bool(value) => Some(u64::from(*value)),
        ExprKind::Cast(ty, value) => constant(value).map(|n| {
            if *ty == Type::Bool {
                u64::from(n != 0)
            } else {
                n
            }
        }),
        ExprKind::Unary(op, e) => {
            let value = constant(e)?;
            Some(match *op {
                "+" => value,
                "-" => value.wrapping_neg(),
                "!" => u64::from(value == 0),
                "~" => !value,
                _ => return None,
            })
        }
        ExprKind::Binary(op, left, right) => {
            let a = constant(left)?;
            let b = constant(right)?;
            Some(match *op {
                "+" => a.wrapping_add(b),
                "-" => a.wrapping_sub(b),
                "*" => a.wrapping_mul(b),
                "&" => a & b,
                "|" => a | b,
                "^" => a ^ b,
                "<<" => a.wrapping_shl((b & 63) as u32),
                ">>" if left.ty == Type::U64 => a.wrapping_shr((b & 63) as u32),
                ">>" => (a as i64).wrapping_shr((b & 63) as u32) as u64,
                ">>>" => a.wrapping_shr((b & 63) as u32),
                "/" if left.ty == Type::U64 => a.checked_div(b)?,
                "%" if left.ty == Type::U64 => a.checked_rem(b)?,
                "/" => (a as i64).checked_div(b as i64)? as u64,
                "%" => (a as i64).checked_rem(b as i64)? as u64,
                "==" => u64::from(a == b),
                "!=" => u64::from(a != b),
                "<" if left.ty == Type::U64 => u64::from(a < b),
                ">" if left.ty == Type::U64 => u64::from(a > b),
                "<=" if left.ty == Type::U64 => u64::from(a <= b),
                ">=" if left.ty == Type::U64 => u64::from(a >= b),
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
    // All code/data references are RIP-relative or RVAs, so there are no
    // absolute addresses to fix. A valid block of ABSOLUTE padding entries
    // lets the loader rebase the position-independent image, including when
    // mandatory ASLR requires a relocation directory.
    dword(&mut b, reloc_raw, TEXT);
    dword(&mut b, reloc_raw + 4, 12);
    let i = 0x10400;
    dword(&mut b, i, IMPORTS + 0x40);
    dword(&mut b, i + 12, IMPORTS + 0x70);
    dword(&mut b, i + 16, IMPORTS + 0x40);
    for (at, rva) in [(0x40, 0x80), (0x48, 0x90), (0x50, 0xa0)] {
        qword(&mut b, i + at, (IMPORTS + rva) as u64);
    }
    for (at, text) in [
        (0x70, &b"KERNEL32.DLL\0"[..]),
        (0x82, &b"GetStdHandle\0"[..]),
        (0x92, &b"WriteFile\0"[..]),
        (0xa2, &b"ExitProcess\0"[..]),
    ] {
        b[i + at..i + at + text.len()].copy_from_slice(text);
    }
    b
}
