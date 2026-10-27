use crate::{
    Diagnostic,
    lexer::{self, Keyword, Kind, Token},
};
use std::{collections::HashMap, ops::Range, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Type {
    I64,
    U64,
    Bool,
    Str,
    Array(Arc<Type>, usize),
    Record(Arc<Record>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub name: String,
    pub fields: Vec<(String, Type)>,
    pub slots: usize,
    pub depth: usize,
}
impl Type {
    pub fn aggregate(&self) -> bool {
        matches!(self, Self::Array(..) | Self::Record(_))
    }
    pub fn array(element: Self, len: usize) -> Option<Self> {
        if !(1..=256).contains(&len)
            || element.slots().checked_mul(len)? > 256
            || element.depth() >= 32
        {
            return None;
        }
        Some(Self::Array(Arc::new(element), len))
    }
    pub fn element(&self) -> Option<Self> {
        if let Self::Array(element, _) = self {
            Some((**element).clone())
        } else {
            None
        }
    }
    pub fn len(&self) -> Option<usize> {
        if let Self::Array(_, len) = self {
            Some(*len)
        } else {
            None
        }
    }
    pub fn depth(&self) -> usize {
        match self {
            Self::Record(record) => record.depth,
            Self::Array(element, _) => element.depth() + 1,
            _ => 0,
        }
    }
    pub fn slots(&self) -> usize {
        if let Self::Array(element, len) = self {
            element.slots() * len
        } else if let Self::Record(record) = self {
            record.slots
        } else {
            1
        }
    }
    pub fn integer(&self) -> bool {
        matches!(self, Self::I64 | Self::U64)
    }
}
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Self::Record(record) = self {
            return f.write_str(&record.name);
        }
        if let Self::Array(_, len) = self {
            return write!(f, "[{}; {len}]", self.element().expect("array element"));
        }
        f.write_str(match self {
            Self::I64 => "i64",
            Self::U64 => "u64",
            Self::Bool => "bool",
            Self::Str => "str",
            Self::Array(..) | Self::Record(_) => unreachable!(),
        })
    }
}
#[derive(Debug)]
pub(crate) struct Param {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug)]
pub(crate) struct Program {
    pub functions: Vec<Function>,
}
#[derive(Debug)]
pub(crate) struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub result: Type,
    pub body: Vec<Stmt>,
    pub span: Range<usize>,
}
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: String,
    pub parameters: usize,
    pub signature: String,
    pub span: Range<usize>,
}
impl From<&Function> for FunctionInfo {
    fn from(f: &Function) -> Self {
        Self {
            name: f.name.clone(),
            parameters: f.params.len(),
            signature: format!(
                "{}({}) -> {}",
                f.name,
                f.params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.ty))
                    .collect::<Vec<_>>()
                    .join(", "),
                f.result
            ),
            span: f.span.clone(),
        }
    }
}
#[derive(Debug)]
pub(crate) struct Stmt {
    pub kind: StmtKind,
    pub span: Range<usize>,
}
#[derive(Debug)]
pub(crate) enum StmtKind {
    Let(String, Option<Type>, Expr),
    Assign(String, Expr),
    IndexAssign(Expr, Expr),
    Expr(Expr),
    Return(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    Block(Vec<Stmt>),
    Builtin(Keyword, Expr),
    Break,
    Continue,
}
#[derive(Debug)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub span: Range<usize>,
    depth: usize,
    pub ty: Type,
}
#[derive(Debug)]
pub(crate) enum ExprKind {
    Number(u64),
    Bool(bool),
    String(String),
    Array(Vec<Expr>),
    Repeat(Box<Expr>, usize),
    Record(Arc<Record>, Vec<(String, Expr)>),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Len(Box<Expr>),
    Cast(Type, Box<Expr>),
    Var(String),
    Call(String, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
}

pub(crate) fn is_place(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Var(_) => true,
        ExprKind::Field(base, _) | ExprKind::Index(base, _) => is_place(base),
        _ => false,
    }
}

pub fn analyze(source: &str) -> Result<Vec<FunctionInfo>, Diagnostic> {
    let program = parse(source, lexer::lex(source)?)?;
    // Include semantic/code-generation checks so the IDE reports the same errors as CLI.
    crate::codegen::generate(source, &program)?;
    Ok(program.functions.iter().map(FunctionInfo::from).collect())
}

pub(crate) fn parse(source: &str, tokens: Vec<Token>) -> Result<Program, Diagnostic> {
    let mut program = Parser {
        source,
        tokens,
        at: 0,
        depth: 0,
        records: HashMap::new(),
        pending_records: HashMap::new(),
        resolving_records: Vec::new(),
    }
    .program()?;
    crate::semantics::check(source, &mut program)?;
    Ok(program)
}
struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
    records: HashMap<String, Arc<Record>>,
    pending_records: HashMap<String, usize>,
    resolving_records: Vec<String>,
}
impl Parser<'_> {
    fn token(&self) -> &Token {
        &self.tokens[self.at]
    }
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(self.source, self.token().span.clone(), message)
    }
    fn sym(&mut self, symbol: &str) -> bool {
        if matches!(self.token().kind,Kind::Sym(s) if s == symbol) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn kw(&mut self, keyword: Keyword) -> bool {
        if self.token().kind == Kind::Keyword(keyword) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, symbol: &str) -> Result<(), Diagnostic> {
        if self.sym(symbol) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{symbol}'")))
        }
    }
    fn name(&mut self) -> Result<String, Diagnostic> {
        if self.token().kind != Kind::Ident {
            return Err(self.error("expected identifier"));
        }
        let name = self.source[self.token().span.clone()].to_owned();
        self.at += 1;
        Ok(name)
    }
    fn type_name(&mut self) -> Result<Type, Diagnostic> {
        self.enter()?;
        let result = self.parse_type();
        self.depth -= 1;
        result
    }
    fn parse_type(&mut self) -> Result<Type, Diagnostic> {
        if self.sym("[") {
            let element = self.type_name()?;
            self.expect(";")?;
            let Kind::Number(len @ 1..=256) = self.token().kind else {
                return Err(self.error("array length must be a literal from 1 to 256"));
            };
            self.at += 1;
            self.expect("]")?;
            return Type::array(element, len as usize)
                .ok_or_else(|| self.error("array layout limit exceeded (256 slots or 32 levels)"));
        }
        if self.token().kind == Kind::Ident {
            let span = self.token().span.clone();
            let name = self.name()?;
            return self.resolve_record(&name, span).map(Type::Record);
        }
        let ty = match self.token().kind {
            Kind::Keyword(Keyword::I64) => Type::I64,
            Kind::Keyword(Keyword::U64) => Type::U64,
            Kind::Keyword(Keyword::Bool) => Type::Bool,
            Kind::Keyword(Keyword::Str) => Type::Str,
            _ => return Err(self.error("expected type i64, u64, bool or str")),
        };
        self.at += 1;
        Ok(ty)
    }
    fn enter(&mut self) -> Result<(), Diagnostic> {
        self.depth += 1;
        if self.depth > 96 {
            Err(self.error("syntax nesting limit exceeded"))
        } else {
            Ok(())
        }
    }
    fn record_definition(&mut self) -> Result<Arc<Record>, Diagnostic> {
        let name = self.name()?;
        self.expect("{")?;
        let mut fields = Vec::new();
        while !self.sym("}") {
            let field = self.name()?;
            if fields.iter().any(|(name, _)| name == &field) {
                return Err(self.error(format!("field already defined: {field}")));
            }
            self.expect(":")?;
            let ty = self.type_name()?;
            fields.push((field, ty));
            if fields.len() > 256 {
                return Err(self.error("struct field limit exceeded (256)"));
            }
            if self.sym("}") {
                break;
            }
            self.expect(",")?;
        }
        if fields.is_empty() {
            return Err(self.error("struct must have at least one field"));
        }
        let slots: usize = fields.iter().map(|(_, ty)| ty.slots()).sum();
        if slots > 256 {
            return Err(self.error("struct layout limit exceeded (256 slots)"));
        }
        let depth = fields.iter().map(|(_, ty)| ty.depth()).max().unwrap_or(0) + 1;
        if depth > 32 {
            return Err(self.error("struct nesting limit exceeded (32)"));
        }
        Ok(Arc::new(Record {
            name,
            fields,
            slots,
            depth,
        }))
    }
    fn resolve_record(
        &mut self,
        name: &str,
        span: Range<usize>,
    ) -> Result<Arc<Record>, Diagnostic> {
        if let Some(record) = self.records.get(name) {
            return Ok(record.clone());
        }
        let Some(&start) = self.pending_records.get(name) else {
            return Err(Diagnostic::new(
                self.source,
                span,
                format!("unknown type: {name}"),
            ));
        };
        if self.resolving_records.iter().any(|active| active == name) {
            return Err(Diagnostic::new(
                self.source,
                span,
                format!("cyclic struct layout: {name}"),
            ));
        }
        if self.resolving_records.len() >= 32 {
            return Err(Diagnostic::new(
                self.source,
                span,
                "struct nesting limit exceeded (32)",
            ));
        }
        let saved = self.at;
        self.at = start;
        self.resolving_records.push(name.to_owned());
        let record = self.record_definition()?;
        self.resolving_records.pop();
        self.at = saved;
        self.records.insert(name.to_owned(), record.clone());
        Ok(record)
    }
    // Collect declaration locations first, then resolve the bounded acyclic layouts.
    fn collect_records(&mut self) -> Result<(), Diagnostic> {
        let mut braces = 0usize;
        let mut names = Vec::new();
        while self.token().kind != Kind::Eof {
            if braces == 0 && self.kw(Keyword::Struct) {
                let start = self.at;
                let name = self.name()?;
                if self.pending_records.insert(name.clone(), start).is_some() {
                    return Err(self.error("struct already defined"));
                }
                names.push(name);
                if names.len() > 128 {
                    return Err(self.error("struct limit exceeded (128)"));
                }
                self.expect("{")?;
                braces = 1;
            } else {
                match self.token().kind {
                    Kind::Sym("{") => braces += 1,
                    Kind::Sym("}") => braces = braces.saturating_sub(1),
                    _ => {}
                }
                self.at += 1;
            }
        }
        for name in names {
            self.resolve_record(&name, 0..0)?;
        }
        self.at = 0;
        Ok(())
    }
    fn program(mut self) -> Result<Program, Diagnostic> {
        self.collect_records()?;
        let mut functions: Vec<Function> = Vec::new();
        while self.token().kind != Kind::Eof {
            if self.kw(Keyword::Struct) {
                self.record_definition()?;
                continue;
            }
            let start = self.token().span.start;
            if !self.kw(Keyword::Fn) {
                return Err(self.error("expected function definition"));
            }
            let name = self.name()?;
            if functions.iter().any(|f| f.name == name) {
                return Err(self.error("function already defined"));
            }
            self.expect("(")?;
            let mut params: Vec<Param> = Vec::new();
            if !self.sym(")") {
                loop {
                    let name = self.name()?;
                    if params.iter().any(|p| p.name == name) {
                        return Err(self.error("parameter already defined"));
                    }
                    let ty = if self.sym(":") {
                        self.type_name()?
                    } else {
                        Type::I64
                    };
                    params.push(Param { name, ty });
                    if params.len() > 32 {
                        return Err(self.error("maximum 32 function parameters"));
                    }
                    if self.sym(")") {
                        break;
                    }
                    self.expect(",")?;
                }
            }
            let result = if self.sym("->") {
                self.type_name()?
            } else {
                Type::I64
            };
            let body = self.block()?;
            if result.aggregate() {
                return Err(self
                    .error("array return values and struct return values are not supported yet"));
            }
            functions.push(Function {
                name,
                params,
                result,
                body,
                span: start..self.tokens[self.at - 1].span.end,
            });
            if functions.len() > 256 {
                return Err(self.error("function limit exceeded"));
            }
        }
        match functions.iter().find(|f| f.name == "main") {
            None => return Err(self.error("fn main() is required")),
            Some(f) if !f.params.is_empty() => {
                return Err(self.error("main must have no parameters"));
            }
            Some(f) if f.result != Type::I64 => return Err(self.error("main must return i64")),
            _ => {}
        }
        Ok(Program { functions })
    }
    fn block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        self.enter()?;
        self.expect("{")?;
        let mut body = Vec::new();
        while !self.sym("}") {
            if self.token().kind == Kind::Eof {
                return Err(self.error("expected '}' before end of file"));
            }
            if self.sym(";") {
                continue;
            }
            body.push(self.statement()?);
        }
        self.depth -= 1;
        Ok(body)
    }
    fn condition(&mut self) -> Result<Expr, Diagnostic> {
        self.expect("(")?;
        let e = self.expr(1)?;
        self.expect(")")?;
        Ok(e)
    }
    fn statement(&mut self) -> Result<Stmt, Diagnostic> {
        self.enter()?;
        let start = self.token().span.start;
        let kind = if self.kw(Keyword::Let) {
            let name = self.name()?;
            let ty = if self.sym(":") {
                Some(self.type_name()?)
            } else {
                None
            };
            self.expect("=")?;
            StmtKind::Let(name, ty, self.expr(1)?)
        } else if self.kw(Keyword::Return) {
            StmtKind::Return(self.expr(1)?)
        } else if self.kw(Keyword::If) {
            let condition = self.condition()?;
            let yes = self.block()?;
            let no = if self.kw(Keyword::Else) {
                if self.token().kind == Kind::Keyword(Keyword::If) {
                    vec![self.statement()?]
                } else {
                    self.block()?
                }
            } else {
                Vec::new()
            };
            StmtKind::If(condition, yes, no)
        } else if self.kw(Keyword::While) {
            let cond = self.condition()?;
            StmtKind::While(cond, self.block()?)
        } else if self.kw(Keyword::Break) {
            StmtKind::Break
        } else if self.kw(Keyword::Continue) {
            StmtKind::Continue
        } else if self.token().kind == Kind::Sym("{") {
            StmtKind::Block(self.block()?)
        } else if let Kind::Keyword(
            k @ (Keyword::Printi
            | Keyword::Printc
            | Keyword::Exit
            | Keyword::Print
            | Keyword::Println),
        ) = self.token().kind
        {
            self.at += 1;
            self.expect("(")?;
            let arg = self.expr(1)?;
            self.expect(")")?;
            StmtKind::Builtin(k, arg)
        } else if self.token().kind == Kind::Ident
            && self
                .tokens
                .get(self.at + 1)
                .is_some_and(|t| t.kind == Kind::Sym("="))
        {
            let name = self.name()?;
            self.expect("=")?;
            StmtKind::Assign(name, self.expr(1)?)
        } else {
            let value = self.expr(1)?;
            if self.sym("=") {
                if !matches!(value.kind, ExprKind::Index(..) | ExprKind::Field(..)) {
                    return Err(self.error("assignment target must be a variable or array element"));
                }
                StmtKind::IndexAssign(value, self.expr(1)?)
            } else {
                StmtKind::Expr(value)
            }
        };
        self.sym(";");
        self.depth -= 1;
        Ok(Stmt {
            kind,
            span: start..self.tokens[self.at.saturating_sub(1)].span.end,
        })
    }
    fn expr(&mut self, min: u8) -> Result<Expr, Diagnostic> {
        self.enter()?;
        let mut left = self.atom()?;
        while let Kind::Sym(op) = self.token().kind {
            let precedence = match op {
                "||" => 1,
                "&&" => 2,
                "|" => 3,
                "^" => 4,
                "&" => 5,
                "==" | "!=" | "<" | ">" | "<=" | ">=" => 6,
                "<<" | ">>" | ">>>" => 7,
                "+" | "-" => 8,
                "*" | "/" | "%" => 9,
                _ => 0,
            };
            if precedence < min {
                break;
            }
            self.at += 1;
            let right = self.expr(precedence + 1)?;
            let span = left.span.start..right.span.end;
            let depth = left.depth.max(right.depth) + 1;
            if depth > 96 {
                return Err(self.error("expression tree depth limit exceeded"));
            }
            left = Expr {
                kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
                span,
                depth,
                ty: Type::I64,
            };
        }
        self.depth -= 1;
        Ok(left)
    }
    fn atom(&mut self) -> Result<Expr, Diagnostic> {
        let mut value = self.primary()?;
        loop {
            if self.sym(".") {
                let field = self.name()?;
                let depth = value.depth + 1;
                if depth > 96 {
                    return Err(self.error("expression tree depth limit exceeded"));
                }
                let span = value.span.start..self.tokens[self.at - 1].span.end;
                value = Expr {
                    kind: ExprKind::Field(Box::new(value), field),
                    span,
                    depth,
                    ty: Type::I64,
                };
                continue;
            }
            if !self.sym("[") {
                break;
            }
            let index = self.expr(1)?;
            self.expect("]")?;
            let depth = value.depth.max(index.depth) + 1;
            if depth > 96 {
                return Err(self.error("expression tree depth limit exceeded"));
            }
            let span = value.span.start..self.tokens[self.at - 1].span.end;
            value = Expr {
                kind: ExprKind::Index(Box::new(value), Box::new(index)),
                span,
                depth,
                ty: Type::I64,
            };
        }
        Ok(value)
    }
    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        self.enter()?;
        let token = self.token().clone();
        let kind = match token.kind {
            Kind::String => {
                self.at += 1;
                let (_, value) = crate::strings::literal(&self.source[token.span.clone()]);
                ExprKind::String(value.map_err(|message| self.error(message))?)
            }
            Kind::Sym("[") => {
                self.at += 1;
                if self.sym("]") {
                    ExprKind::Array(Vec::new())
                } else {
                    let first = self.expr(1)?;
                    if self.sym(";") {
                        let Kind::Number(count) = self.token().kind else {
                            return Err(self
                                .error("array repeat length must be an integer literal (1..256)"));
                        };
                        if !(1..=256).contains(&count) {
                            return Err(self.error("array repeat length must be in 1..256"));
                        }
                        self.at += 1;
                        self.expect("]")?;
                        ExprKind::Repeat(Box::new(first), count as usize)
                    } else {
                        let mut values = vec![first];
                        while !self.sym("]") {
                            self.expect(",")?;
                            if self.sym("]") {
                                break;
                            }
                            values.push(self.expr(1)?);
                            if values.len() > 256 {
                                return Err(
                                    self.error("array literal limit exceeded (256 elements)")
                                );
                            }
                        }
                        ExprKind::Array(values)
                    }
                }
            }
            Kind::Keyword(Keyword::Len) => {
                self.at += 1;
                self.expect("(")?;
                let value = self.expr(1)?;
                self.expect(")")?;
                ExprKind::Len(Box::new(value))
            }
            Kind::Number(n) => {
                self.at += 1;
                ExprKind::Number(n)
            }
            Kind::Keyword(Keyword::True | Keyword::False) => {
                self.at += 1;
                ExprKind::Bool(token.kind == Kind::Keyword(Keyword::True))
            }
            Kind::Keyword(Keyword::I64 | Keyword::U64 | Keyword::Bool) => {
                let ty = self.type_name()?;
                self.expect("(")?;
                let value = self.expr(1)?;
                self.expect(")")?;
                ExprKind::Cast(ty, Box::new(value))
            }
            Kind::Ident => {
                let name = self.name()?;
                if let Some(record) = self.records.get(&name).cloned()
                    && self.sym("{")
                {
                    let mut fields = Vec::new();
                    while !self.sym("}") {
                        let field = self.name()?;
                        self.expect(":")?;
                        fields.push((field, self.expr(1)?));
                        if fields.len() > 256 {
                            return Err(self.error("struct initializer field limit exceeded (256)"));
                        }
                        if self.sym("}") {
                            break;
                        }
                        self.expect(",")?;
                    }
                    ExprKind::Record(record, fields)
                } else if self.sym("(") {
                    let mut args = Vec::new();
                    if !self.sym(")") {
                        loop {
                            args.push(self.expr(1)?);
                            if args.len() > 32 {
                                return Err(self.error("maximum 32 function arguments"));
                            }
                            if self.sym(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    ExprKind::Call(name, args)
                } else {
                    ExprKind::Var(name)
                }
            }
            Kind::Sym(op @ ("+" | "-" | "!" | "~")) => {
                self.at += 1;
                ExprKind::Unary(op, Box::new(self.atom()?))
            }
            Kind::Sym("(") => {
                self.at += 1;
                let e = self.expr(1)?;
                self.expect(")")?;
                self.depth -= 1;
                return Ok(e);
            }
            _ => return Err(self.error("expected expression")),
        };
        self.depth -= 1;
        let depth = match &kind {
            ExprKind::Unary(_, e)
            | ExprKind::Cast(_, e)
            | ExprKind::Len(e)
            | ExprKind::Repeat(e, _) => e.depth + 1,
            ExprKind::Field(e, _) => e.depth + 1,
            ExprKind::Record(_, fields) => {
                fields.iter().map(|(_, e)| e.depth).max().unwrap_or(0) + 1
            }
            ExprKind::Call(_, args) | ExprKind::Array(args) => {
                args.iter().map(|e| e.depth).max().unwrap_or(0) + 1
            }
            _ => 1,
        };
        if depth > 96 {
            return Err(self.error("expression tree depth limit exceeded"));
        }
        Ok(Expr {
            kind,
            span: token.span.start..self.tokens[self.at.saturating_sub(1)].span.end,
            depth,
            ty: Type::I64,
        })
    }
}
