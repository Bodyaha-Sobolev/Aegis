//! Type checking precedes optimization, including unreachable/short-circuited code.
use crate::{
    Diagnostic,
    lexer::Keyword,
    parser::{Expr, ExprKind, Program, Stmt, StmtKind, Type},
};
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
        vars: Vec::new(),
        result: Type::I64,
    };
    for function in &mut program.functions {
        checker.vars.clear();
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
    }
    Ok(())
}
struct Checker<'a> {
    source: &'a str,
    signatures: HashMap<String, (Vec<Type>, Type)>,
    vars: Vec<(String, Type)>,
    result: Type,
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
                    self.expr(value, Some(ty))?;
                }
                StmtKind::Return(value) => {
                    self.expr(value, Some(self.result.clone()))?;
                }
                StmtKind::Expr(value) => {
                    self.expr(value, None)?;
                }
                StmtKind::Builtin(kind, value) => {
                    if matches!(kind, Keyword::Print | Keyword::Println) {
                        self.expr(value, Some(Type::Str))?;
                    } else {
                        self.numeric(value, None)?;
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
                self.scalar(value, Some(field_type.ty()))?;
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
            let element = self.scalar(value, expected.as_ref().and_then(|ty| ty.element()))?;
            let ty = Type::array(element, *count).expect("scalar array element");
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
            if values.is_empty() {
                return Err(self.error(&expr.span, "empty arrays are not supported yet"));
            }
            if let Some(expected) = expected.as_ref()
                && expected.element().is_none()
            {
                return Err(self.error(&expr.span, "type mismatch: expected scalar, found array"));
            }
            let element = expected
                .as_ref()
                .and_then(|ty| ty.element())
                .or_else(|| values.iter().find_map(|v| self.hint(v)))
                .unwrap_or(Type::I64);
            if element.aggregate() {
                return Err(self.error(&expr.span, "nested arrays are not supported yet"));
            }
            for value in values.iter_mut() {
                self.scalar(value, Some(element.clone()))?;
            }
            let ty = Type::array(element, values.len()).expect("scalar array element");
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
            ExprKind::Bool(_) => Some(Type::Bool),
            ExprKind::String(_) => Some(Type::Str),
            ExprKind::Array(_) | ExprKind::Repeat(..) => None,
            ExprKind::Record(record, _) => Some(Type::Record(record.clone())),
            ExprKind::Field(base, name) => match self.hint(base) {
                Some(Type::Record(record)) => record
                    .fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, ty)| ty.ty()),
                _ => None,
            },
            ExprKind::Index(target, _) => match self.hint(target) {
                Some(Type::Str) => Some(Type::U64),
                other => other.and_then(|ty| ty.element()),
            },
            ExprKind::Len(_) => Some(Type::I64),
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
        let ty = match &mut expr.kind {
            ExprKind::String(_) => Type::Str,
            ExprKind::Number(_) => expected
                .clone()
                .filter(|t| t.integer())
                .unwrap_or(Type::I64),
            ExprKind::Array(_) | ExprKind::Repeat(..) | ExprKind::Record(..) => {
                return Err(self.error(
                    &expr.span,
                    "aggregate literals require a local declaration, whole-value assignment or aggregate argument",
                ));
            }
            ExprKind::Field(base, name) => {
                let ty = self.expr(base, None)?;
                if !matches!(base.kind, ExprKind::Var(_)) {
                    return Err(self.error(&base.span, "field access requires a named struct"));
                }
                let Type::Record(record) = ty else {
                    return Err(self.error(&base.span, "field access requires a struct"));
                };
                record
                    .fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .map(|(_, ty)| ty.ty())
                    .ok_or_else(|| {
                        self.error(
                            &expr.span,
                            format!("unknown field {name} in {}", record.name),
                        )
                    })?
            }
            ExprKind::Len(value) => {
                let ty = self.expr(value, None)?;
                if ty != Type::Str
                    && (ty.element().is_none() || !matches!(value.kind, ExprKind::Var(_)))
                {
                    return Err(self.error(&value.span, "len requires a string or named array"));
                }
                Type::I64
            }
            ExprKind::Index(target, index) => {
                let ty = self.expr(target, None)?;
                let element = if ty == Type::Str {
                    Type::U64
                } else {
                    if !matches!(target.kind, ExprKind::Var(_)) {
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
                let ty = self.numeric(
                    value,
                    if *op == "!" {
                        None
                    } else {
                        hint.or(expected.clone())
                    },
                )?;
                if *op == "!" {
                    Type::Bool
                } else {
                    if ty == Type::Bool {
                        return Err(self.error(
                            &expr.span,
                            "arithmetic and bitwise operators require integers",
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
                    if left_ty == Type::Bool || right_ty == Type::Bool {
                        return Err(self.error(&expr.span, "shift operators require integers"));
                    }
                    left_ty
                } else {
                    let comparison = matches!(*op, "==" | "!=" | "<" | ">" | "<=" | ">=");
                    let common = self
                        .hint(left)
                        .or_else(|| self.hint(right))
                        .or(if comparison { None } else { expected.clone() })
                        .unwrap_or(Type::I64);
                    let left_ty = self.scalar(left, Some(common.clone()))?;
                    self.scalar(right, Some(left_ty.clone()))?;
                    if left_ty == Type::Str && !matches!(*op, "==" | "!=") {
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
