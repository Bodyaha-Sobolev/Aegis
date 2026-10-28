//! Type checking precedes optimization, including unreachable/short-circuited code.
use crate::{
    Diagnostic,
    lexer::Keyword,
    parser::{Expr, ExprKind, Program, Stmt, StmtKind, Type, is_place},
};
use std::sync::Arc;
use std::{collections::HashMap, ops::Range};

pub(crate) fn check(source: &str, program: &mut Program) -> Result<(), Diagnostic> {
    let signatures = program
        .functions
        .iter()
        .map(|f| {
            (
                f.name.clone(),
                (
                    f.params.iter().map(|p| p.ty.clone()).collect(),
                    f.result.clone(),
                ),
            )
        })
        .collect();
    let mut checker = Checker {
        source,
        signatures,
        externals: program
            .functions
            .iter()
            .filter(|f| f.external.is_some())
            .map(|f| f.name.clone())
            .collect(),
        unsafe_depth: 0,
        vars: Vec::new(),
        result: Type::I64,
        negate_literal: false,
    };
    for function in &mut program.functions {
        if function.external.is_some() {
            continue;
        }
        checker.vars.clear();
        checker.unsafe_depth = 0;
        checker.result = function.result.clone();
        for param in &function.params {
            if checker.vars.iter().map(|(_, t)| t.slots()).sum::<usize>() + param.ty.slots() > 256 {
                return Err(checker.error(
                    &function.span,
                    "local variable limit exceeded by function parameters",
                ));
            }
            checker.vars.push((param.name.clone(), param.ty.clone()));
        }
        checker.block(&mut function.body)?;
        if function.result.aggregate() && !returns(&function.body) {
            return Err(checker.error(
                &function.span,
                "aggregate result requires return on every path",
            ));
        }
    }
    Ok(())
}
// Conservative definite-return analysis: loops are not assumed to terminate or return.
fn returns(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::Block(body) | StmtKind::Unsafe(body) => returns(body),
        StmtKind::If(_, yes, no) => returns(yes) && returns(no),
        _ => false,
    })
}
struct Checker<'a> {
    source: &'a str,
    signatures: HashMap<String, (Vec<Type>, Type)>,
    externals: std::collections::HashSet<String>,
    unsafe_depth: usize,
    vars: Vec<(String, Type)>,
    result: Type,
    // Set by unary minus so its immediate literal may reach the type minimum.
    negate_literal: bool,
}
impl Checker<'_> {
    fn error(&self, span: &Range<usize>, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(self.source, span.clone(), message)
    }
    fn variable(&self, name: &str, span: &Range<usize>) -> Result<Type, Diagnostic> {
        self.vars
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, ty)| ty.clone())
            .ok_or_else(|| self.error(span, format!("undefined variable: {name}")))
    }
    fn block(&mut self, body: &mut [Stmt]) -> Result<(), Diagnostic> {
        let scope = self.vars.len();
        for stmt in body {
            match &mut stmt.kind {
                StmtKind::Let(name, annotation, value) => {
                    let ty = self.aggregate_value(value, annotation.clone())?;
                    if self.vars.iter().any(|(n, _)| n == name) {
                        return Err(
                            self.error(&stmt.span, format!("variable already defined: {name}"))
                        );
                    }
                    if self.vars.iter().map(|(_, t)| t.slots()).sum::<usize>() + ty.slots() > 256 {
                        return Err(self.error(&stmt.span, "local variable limit exceeded"));
                    }
                    self.vars.push((name.clone(), ty));
                }
                StmtKind::Assign(name, value) => {
                    let ty = self.variable(name, &stmt.span)?;
                    self.aggregate_value(value, Some(ty))?;
                }
                StmtKind::IndexAssign(target, value) => {
                    let ty = self.expr(target, None)?;
                    if let ExprKind::Index(base, _) = &target.kind
                        && base.ty == Type::Str
                    {
                        return Err(self.error(&target.span, "string bytes are immutable"));
                    }
                    if let ExprKind::Index(base, _) = &target.kind
                        && base.ty.dyn_array()
                        && !is_place(base)
                    {
                        return Err(self.error(&base.span, "assignment requires a named array"));
                    }
                    self.aggregate_value(value, Some(ty))?;
                }
                StmtKind::Return(value) => {
                    self.aggregate_value(value, Some(self.result.clone()))?;
                }
                StmtKind::Expr(value) => {
                    self.expr(value, None)?;
                }
                StmtKind::Builtin(kind, value) => {
                    if matches!(kind, Keyword::Print | Keyword::Println) {
                        self.expr(value, Some(Type::Str))?;
                    } else {
                        let ty = self.numeric(value, None)?;
                        if !matches!(kind, Keyword::Printi) && !ty.integer() {
                            return Err(
                                self.error(&stmt.span, "printc and exit require an integer value")
                            );
                        }
                    }
                }
                StmtKind::If(condition, yes, no) => {
                    self.numeric(condition, None)?;
                    self.block(yes)?;
                    self.block(no)?;
                }
                StmtKind::While(condition, body) => {
                    self.numeric(condition, None)?;
                    self.block(body)?;
                }
                StmtKind::Block(body) => self.block(body)?,
                StmtKind::Unsafe(body) => {
                    self.unsafe_depth += 1;
                    let result = self.block(body);
                    self.unsafe_depth -= 1;
                    result?;
                }
                StmtKind::Break | StmtKind::Continue => {}
            }
        }
        self.vars.truncate(scope);
        Ok(())
    }
    fn scalar(&mut self, expr: &mut Expr, expected: Option<Type>) -> Result<Type, Diagnostic> {
        let ty = self.expr(expr, expected)?;
        if ty.aggregate() {
            return Err(self.error(&expr.span, "expected a scalar value, found aggregate"));
        }
        Ok(ty)
    }
    fn aggregate_value(
        &mut self,
        expr: &mut Expr,
        expected: Option<Type>,
    ) -> Result<Type, Diagnostic> {
        if let ExprKind::Record(record, fields) = &mut expr.kind {
            let ty = Type::Record(record.clone());
            if let Some(expected) = expected.as_ref()
                && *expected != ty
            {
                return Err(self.error(
                    &expr.span,
                    format!("type mismatch: expected {expected}, found {ty}"),
                ));
            }
            let mut seen = std::collections::HashSet::new();
            for (name, value) in fields.iter_mut() {
                if !seen.insert(name.clone()) {
                    return Err(self.error(&value.span, format!("duplicate field: {name}")));
                }
                let Some((_, field_type)) = record.fields.iter().find(|(field, _)| field == name)
                else {
                    return Err(self.error(
                        &value.span,
                        format!("unknown field {name} in {}", record.name),
                    ));
                };
                self.aggregate_value(value, Some(field_type.clone()))?;
            }
            if fields.len() != record.fields.len() {
                return Err(self.error(
                    &expr.span,
                    format!("missing fields in {} initializer", record.name),
                ));
            }
            expr.ty = ty.clone();
            return Ok(ty);
        }
        if let ExprKind::Repeat(value, count) = &mut expr.kind {
            if expected.as_ref().is_some_and(|ty| ty.element().is_none()) {
                return Err(self.error(&expr.span, "type mismatch: expected scalar, found array"));
            }
            let element =
                self.aggregate_value(value, expected.as_ref().and_then(|ty| ty.element()))?;
            let ty = if matches!(expected.as_ref(), Some(Type::Dyn(_))) {
                if element.aggregate() || element.dyn_array() {
                    return Err(self.error(
                        &expr.span,
                        "dynamic array elements must be one-slot scalars or str",
                    ));
                }
                Type::Dyn(Arc::new(element))
            } else {
                Type::array(element, *count).ok_or_else(|| {
                    self.error(
                        &expr.span,
                        "array layout limit exceeded (256 slots or 32 levels)",
                    )
                })?
            };
            if let Some(expected) = expected.as_ref()
                && *expected != ty
            {
                return Err(self.error(
                    &expr.span,
                    format!("type mismatch: expected {expected}, found {ty}"),
                ));
            }
            expr.ty = ty.clone();
            return Ok(ty);
        }
        if let ExprKind::Array(values) = &mut expr.kind {
            if values.is_empty() && !matches!(expected.as_ref(), Some(Type::Dyn(_))) {
                return Err(self.error(&expr.span, "empty arrays are not supported yet"));
            }
            if let Some(expected) = expected.as_ref()
                && expected.element().is_none()
            {
                return Err(self.error(&expr.span, "type mismatch: expected scalar, found array"));
            }
            let dyn_context = matches!(expected.as_ref(), Some(Type::Dyn(_)));
            let element = expected
                .as_ref()
                .and_then(|ty| ty.element())
                .or_else(|| values.iter().find_map(|v| self.hint(v)));
            let element = if let Some(element) = element {
                element
            } else {
                self.aggregate_value(&mut values[0], None)?
            };
            for value in values.iter_mut() {
                self.aggregate_value(value, Some(element.clone()))?;
            }
            let ty = if dyn_context {
                if element.aggregate() || element.dyn_array() {
                    return Err(self.error(
                        &expr.span,
                        "dynamic array elements must be one-slot scalars or str",
                    ));
                }
                Type::Dyn(Arc::new(element))
            } else {
                Type::array(element, values.len()).ok_or_else(|| {
                    self.error(
                        &expr.span,
                        "array layout limit exceeded (256 slots or 32 levels)",
                    )
                })?
            };
            if let Some(expected) = expected.as_ref()
                && *expected != ty
            {
                return Err(self.error(
                    &expr.span,
                    format!("type mismatch: expected {expected}, found {ty}"),
                ));
            }
            expr.ty = ty.clone();
            Ok(ty)
        } else {
            self.expr(expr, expected)
        }
    }
    fn numeric(&mut self, expr: &mut Expr, expected: Option<Type>) -> Result<Type, Diagnostic> {
        let ty = self.scalar(expr, expected)?;
        if ty == Type::Str {
            return Err(self.error(&expr.span, "expected a numeric or bool value, found str"));
        }
        Ok(ty)
    }
    // A literal acquires an integer type from surrounding values/annotations.
    // Named values never change type implicitly.
    fn hint(&self, expr: &Expr) -> Option<Type> {
        match &expr.kind {
            ExprKind::Number(_) => None,
            ExprKind::Float(_) => None,
            ExprKind::Bool(_) => Some(Type::Bool),
            ExprKind::String(_) => Some(Type::Str),
            ExprKind::Array(values) => values
                .iter()
                .find_map(|value| self.hint(value))
                .and_then(|element| Type::array(element, values.len())),
            ExprKind::Repeat(value, count) => self
                .hint(value)
                .and_then(|element| Type::array(element, *count)),
            ExprKind::Record(record, _) => Some(Type::Record(record.clone())),
            ExprKind::Field(base, name) => match self.hint(base) {
                Some(Type::Record(record)) => record
                    .fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, ty)| ty.clone()),
                _ => None,
            },
            ExprKind::Index(target, _) => match self.hint(target) {
                Some(Type::Str) => Some(Type::U64),
                other => other.and_then(|ty| ty.element()),
            },
            ExprKind::Len(_) => Some(Type::I64),
            ExprKind::Push(array, _) => match self.hint(array) {
                Some(Type::Dyn(element)) => Some(Type::Dyn(element)),
                _ => None,
            },
            ExprKind::Var(name) => self
                .vars
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, t)| t.clone()),
            ExprKind::Call(name, _) => self.signatures.get(name).map(|(_, t)| t.clone()),
            ExprKind::Cast(ty, _) => Some(ty.clone()),
            ExprKind::Unary("!", _) => Some(Type::Bool),
            ExprKind::Unary(_, value) => self.hint(value),
            ExprKind::Binary("==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||", _, _) => {
                Some(Type::Bool)
            }
            ExprKind::Binary("<<" | ">>" | ">>>", left, _) => self.hint(left),
            ExprKind::Binary(_, left, right) => self.hint(left).or_else(|| self.hint(right)),
        }
    }
    fn expr(&mut self, expr: &mut Expr, expected: Option<Type>) -> Result<Type, Diagnostic> {
        let hint = self.hint(expr);
        let negated = std::mem::take(&mut self.negate_literal);
        let ty = match &mut expr.kind {
            ExprKind::String(_) => Type::Str,
            ExprKind::Float(_) => expected.clone().filter(|t| t.float()).unwrap_or(Type::F64),
            ExprKind::Number(n) => {
                let ty = expected
                    .clone()
                    .filter(|t| t.integer())
                    .unwrap_or(Type::I64);
                // Positive decimal literals must fit; the negated form may
                // reach the minimum, so -2147483648 is writable without a
                // cast. Hex/binary literals carry bit patterns and may use
                // the full unsigned width of the target type.
                let radix_wide = matches!(
                    self.source.get(expr.span.clone()),
                    Some(text) if text.starts_with("0x") || text.starts_with("0X") || text.starts_with("0b") || text.starts_with("0B")
                );
                let max = match ty {
                    Type::I8 if negated => 1u64 << 7,
                    Type::I8 if radix_wide => u8::MAX as u64,
                    Type::I8 => i8::MAX as u64,
                    Type::I16 if negated => 1u64 << 15,
                    Type::I16 if radix_wide => u16::MAX as u64,
                    Type::I16 => i16::MAX as u64,
                    Type::I32 if negated => 1u64 << 31,
                    Type::I32 if radix_wide => u32::MAX as u64,
                    Type::I32 => i32::MAX as u64,
                    Type::U8 => u8::MAX as u64,
                    Type::U16 => u16::MAX as u64,
                    Type::U32 => u32::MAX as u64,
                    _ => u64::MAX,
                };
                if *n > max {
                    return Err(self.error(
                        &expr.span,
                        format!("integer literal {n} does not fit in {ty}"),
                    ));
                }
                ty
            }
            ExprKind::Array(_) | ExprKind::Repeat(..) | ExprKind::Record(..) => {
                return Err(self.error(
                    &expr.span,
                    "aggregate literals require a local declaration, whole-value assignment or aggregate argument",
                ));
            }
            ExprKind::Field(base, name) => {
                let ty = self.expr(base, None)?;
                if !is_place(base) {
                    return Err(self.error(&base.span, "field access requires a named struct"));
                }
                let Type::Record(record) = ty else {
                    return Err(self.error(&base.span, "field access requires a struct"));
                };
                record
                    .fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, ty)| ty.clone())
                    .ok_or_else(|| {
                        self.error(
                            &expr.span,
                            format!("unknown field {name} in {}", record.name),
                        )
                    })?
            }
            ExprKind::Len(value) => {
                let ty = self.expr(value, None)?;
                if ty.dyn_array() {
                    Type::I64
                } else if ty != Type::Str && (ty.element().is_none() || !is_place(value)) {
                    return Err(self.error(&value.span, "len requires a string or named array"));
                } else {
                    Type::I64
                }
            }
            ExprKind::Push(array, value) => {
                let ty = self.expr(array, None)?;
                let Some(element) = ty.element() else {
                    return Err(self.error(&array.span, "push requires a dynamic array"));
                };
                if !ty.dyn_array() {
                    return Err(self.error(&array.span, "push requires a dynamic array"));
                }
                self.aggregate_value(value, Some(element.clone()))?;
                ty
            }
            ExprKind::Index(target, index) => {
                let ty = self.expr(target, None)?;
                let element = if ty == Type::Str {
                    Type::U64
                } else {
                    if !ty.dyn_array() && !is_place(target) {
                        return Err(
                            self.error(&target.span, "indexing requires a named array or string")
                        );
                    }
                    ty.element().ok_or_else(|| {
                        self.error(&target.span, "indexing requires an array or string")
                    })?
                };
                let index_ty = self.numeric(index, None)?;
                if !index_ty.integer() {
                    return Err(self.error(&index.span, "array index must be an integer"));
                }
                element
            }
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Var(name) => self.variable(name, &expr.span)?,
            ExprKind::Cast(ty, value) => {
                self.numeric(value, None)?;
                ty.clone()
            }
            ExprKind::Call(name, args) => {
                if self.externals.contains(name) && self.unsafe_depth == 0 {
                    return Err(self.error(
                        &expr.span,
                        "external calls require an explicit unsafe block",
                    ));
                }
                let Some((params, result)) = self.signatures.get(name).cloned() else {
                    return Err(self.error(&expr.span, format!("undefined function: {name}")));
                };
                if params.len() != args.len() {
                    return Err(self.error(
                        &expr.span,
                        format!(
                            "wrong number of function arguments for {name}: expected {}",
                            params.len()
                        ),
                    ));
                }
                for (value, ty) in args.iter_mut().zip(params) {
                    self.aggregate_value(value, Some(ty))?;
                }
                result
            }
            ExprKind::Unary(op, value) => {
                let ty = if *op == "!" {
                    self.numeric(value, None)?
                } else {
                    let target = hint.or(expected.clone());
                    if *op == "-" && target.as_ref().is_some_and(|t| t.narrow()) {
                        self.negate_literal = true;
                    }
                    let checked = self.numeric(value, target)?;
                    self.negate_literal = false;
                    checked
                };
                if *op == "!" {
                    Type::Bool
                } else {
                    if (ty == Type::Bool || *op == "~") && !ty.integer() {
                        return Err(self.error(
                            &expr.span,
                            "bitwise operators require integers; use arithmetic on f64",
                        ));
                    }
                    ty
                }
            }
            ExprKind::Binary(op, left, right) => {
                if matches!(*op, "&&" | "||") {
                    self.numeric(left, None)?;
                    self.numeric(right, None)?;
                    Type::Bool
                } else if matches!(*op, "<<" | ">>" | ">>>") {
                    let left_ty =
                        self.numeric(left, hint.or(expected.clone().filter(|t| t.integer())))?;
                    let right_ty = self.numeric(right, None)?;
                    if left_ty == Type::Bool || right_ty == Type::Bool || left_ty.float() {
                        return Err(self.error(&expr.span, "shift operators require integers"));
                    }
                    left_ty
                } else {
                    let comparison = matches!(*op, "==" | "!=" | "<" | ">" | "<=" | ">=");
                    // Float literals default to f64; int literals to i64.
                    // Mixing them still fails the per-operand type checks.
                    let default = if matches!(left.kind, ExprKind::Float(_))
                        || matches!(right.kind, ExprKind::Float(_))
                    {
                        Type::F64
                    } else {
                        Type::I64
                    };
                    let common = self
                        .hint(left)
                        .or_else(|| self.hint(right))
                        .or(if comparison { None } else { expected.clone() })
                        .unwrap_or(default);
                    let left_ty = self.scalar(left, Some(common.clone()))?;
                    self.scalar(right, Some(left_ty.clone()))?;
                    if left_ty == Type::Str && !matches!(*op, "==" | "!=" | "+") {
                        return Err(
                            self.error(&expr.span, "strings support only == and != operators")
                        );
                    }
                    if left_ty == Type::Bool && !matches!(*op, "==" | "!=") {
                        return Err(self.error(
                            &expr.span,
                            "arithmetic, ordering and bitwise operators require integers",
                        ));
                    }
                    if left_ty.float() && !comparison && !matches!(*op, "+" | "-" | "*" | "/") {
                        return Err(self.error(
                            &expr.span,
                            "bitwise and remainder operators require integers",
                        ));
                    }
                    if left_ty.float() && !comparison && !matches!(*op, "+" | "-" | "*" | "/") {
                        return Err(self.error(
                            &expr.span,
                            "bitwise and remainder operators require integers",
                        ));
                    }
                    if comparison { Type::Bool } else { left_ty }
                }
            }
        };
        if let Some(expected) = expected.as_ref()
            && ty != *expected
        {
            return Err(self.error(
                &expr.span,
                format!(
                    "type mismatch: expected {expected}, found {ty}; use an explicit conversion"
                ),
            ));
        }
        expr.ty = ty.clone();
        Ok(ty)
    }
}
