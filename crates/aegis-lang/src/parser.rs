use crate::{
    Diagnostic,
    lexer::{self, Keyword, Kind, Token},
};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Type {
    I64,
    U64,
    I32,
    U32,
    I16,
    U16,
    I8,
    U8,
    F64,
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
        matches!(
            self,
            Self::I64
                | Self::U64
                | Self::I32
                | Self::U32
                | Self::I16
                | Self::U16
                | Self::I8
                | Self::U8
        )
    }
    // Narrow types live in 8-byte slots as canonical values: signed widths are
    // always sign-extended, unsigned always zero-extended. Codegen
    // canonicalizes only at value producers; bitwise & | ^ on canonical
    // operands stay canonical.
    pub fn unsigned(&self) -> bool {
        matches!(self, Self::U64 | Self::U32 | Self::U16 | Self::U8)
    }
    pub fn narrow(&self) -> bool {
        matches!(
            self,
            Self::I32 | Self::U32 | Self::I16 | Self::U16 | Self::I8 | Self::U8
        )
    }
    pub fn bits(&self) -> u32 {
        match self {
            Self::I32 | Self::U32 => 32,
            Self::I16 | Self::U16 => 16,
            Self::I8 | Self::U8 => 8,
            _ => 64,
        }
    }
    pub fn float(&self) -> bool {
        matches!(self, Self::F64)
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
            Self::I32 => "i32",
            Self::U32 => "u32",
            Self::I16 => "i16",
            Self::U16 => "u16",
            Self::I8 => "i8",
            Self::U8 => "u8",
            Self::F64 => "f64",
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
    pub path: Option<std::path::PathBuf>,
    pub name: String,
    pub parameters: usize,
    pub signature: String,
    pub span: Range<usize>,
}
impl From<&Function> for FunctionInfo {
    fn from(f: &Function) -> Self {
        Self {
            path: None,
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
    Float(f64),
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
        module: String::new(),
        items: Vec::new(),
        modules: HashSet::new(),
        function_names: HashSet::new(),
        imports: Vec::new(),
        bindings: HashMap::new(),
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
    pending_records: HashMap<String, (usize, String)>,
    resolving_records: Vec<String>,
    module: String,
    items: Vec<Item>,
    modules: HashSet<String>,
    function_names: HashSet<String>,
    imports: Vec<Import>,
    bindings: HashMap<String, String>,
}
struct Import {
    module: String,
    target: String,
    name: String,
    span: Range<usize>,
}
#[derive(Clone)]
struct Item {
    kind: Keyword,
    start: usize,
    end: usize,
    name: String,
    module: String,
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
    fn qualify(&self, name: &str) -> String {
        if self.module.is_empty() {
            name.to_owned()
        } else {
            format!("{}::{name}", self.module)
        }
    }
    fn path(&mut self) -> Result<String, Diagnostic> {
        let absolute = self.sym("::");
        let mut name = self.name()?;
        let mut count = 1;
        while self.sym("::") {
            count += 1;
            if count > 17 {
                return Err(self.error("path limit exceeded (17 components)"));
            }
            name.push_str("::");
            name.push_str(&self.name()?);
        }
        if absolute {
            name.insert_str(0, "::");
        }
        Ok(name)
    }
    fn lookup(&self, name: &str, record: bool) -> Option<String> {
        let exists = |candidate: &str| {
            if record {
                self.pending_records.contains_key(candidate)
            } else {
                self.function_names.contains(candidate)
            }
        };
        if let Some(root) = name.strip_prefix("::") {
            return exists(root).then(|| root.to_owned());
        }
        let mut scope = self.module.as_str();
        loop {
            let candidate = if scope.is_empty() {
                name.to_owned()
            } else {
                format!("{scope}::{name}")
            };
            if exists(&candidate) {
                return Some(candidate);
            }
            let (head, tail) = name.split_once("::").unwrap_or((name, ""));
            let binding = if scope.is_empty() {
                head.to_owned()
            } else {
                format!("{scope}::{head}")
            };
            if let Some(target) = self.bindings.get(&binding) {
                let expanded = if tail.is_empty() {
                    target.clone()
                } else {
                    format!("{target}::{tail}")
                };
                // An explicit binding shadows outer bindings even when it has
                // the wrong kind. Never silently call an unrelated function.
                return exists(&expanded).then_some(expanded);
            }
            if scope.is_empty() {
                return None;
            }
            scope = scope.rsplit_once("::").map_or("", |(parent, _)| parent);
        }
    }
    fn resolve_imports(&mut self) -> Result<(), Diagnostic> {
        let exists = |name: &str| {
            self.modules.contains(name)
                || self.function_names.contains(name)
                || self.pending_records.contains_key(name)
        };
        for import in &self.imports {
            if exists(&import.name) || self.bindings.contains_key(&import.name) {
                return Err(Diagnostic::new(
                    self.source,
                    import.span.clone(),
                    format!("import name already defined: {}", import.name),
                ));
            }
            // Import targets name declarations, not other imports. This keeps
            // binding independent of source/file order and prevents cycles.
            let resolved = if let Some(root) = import.target.strip_prefix("::") {
                exists(root).then(|| root.to_owned())
            } else {
                let mut scope = import.module.as_str();
                loop {
                    let candidate = if scope.is_empty() {
                        import.target.clone()
                    } else {
                        format!("{scope}::{}", import.target)
                    };
                    if exists(&candidate) {
                        break Some(candidate);
                    }
                    if scope.is_empty() {
                        break None;
                    }
                    scope = scope.rsplit_once("::").map_or("", |(parent, _)| parent);
                }
            };
            let target = resolved.ok_or_else(|| {
                Diagnostic::new(
                    self.source,
                    import.span.clone(),
                    format!("unknown import target: {}", import.target),
                )
            })?;
            self.bindings.insert(import.name.clone(), target);
        }
        Ok(())
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
        if matches!(self.token().kind, Kind::Ident | Kind::Sym("::")) {
            let start = self.token().span.start;
            let name = self.path()?;
            let span = start..self.tokens[self.at - 1].span.end;
            let resolved = self.lookup(&name, true).ok_or_else(|| {
                Diagnostic::new(self.source, span.clone(), format!("unknown type: {name}"))
            })?;
            return self.resolve_record(&resolved, span).map(Type::Record);
        }
        let ty = match self.token().kind {
            Kind::Keyword(Keyword::I64) => Type::I64,
            Kind::Keyword(Keyword::U64) => Type::U64,
            Kind::Keyword(Keyword::I32) => Type::I32,
            Kind::Keyword(Keyword::U32) => Type::U32,
            Kind::Keyword(Keyword::I16) => Type::I16,
            Kind::Keyword(Keyword::U16) => Type::U16,
            Kind::Keyword(Keyword::I8) => Type::I8,
            Kind::Keyword(Keyword::U8) => Type::U8,
            Kind::Keyword(Keyword::F64) => Type::F64,
            Kind::Keyword(Keyword::Bool) => Type::Bool,
            Kind::Keyword(Keyword::Str) => Type::Str,
            _ => {
                return Err(self.error(
                    "expected type i64, u64, i32, u32, i16, u16, i8, u8, f64, bool or str",
                ));
            }
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
        let short = self.name()?;
        let name = self.qualify(&short);
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
        let Some((start, module)) = self.pending_records.get(name).cloned() else {
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
        let saved_module = std::mem::replace(&mut self.module, module);
        self.resolving_records.push(name.to_owned());
        let record = self.record_definition()?;
        self.resolving_records.pop();
        self.at = saved;
        self.module = saved_module;
        self.records.insert(name.to_owned(), record.clone());
        Ok(record)
    }
    // Index all declarations before resolving any references. Scanning bodies is
    // iterative; module recursion is separately bounded and never reads files.
    fn collect_items(&mut self, nested: bool, depth: usize) -> Result<(), Diagnostic> {
        loop {
            if self.token().kind == Kind::Eof {
                if nested {
                    return Err(self.error("expected '}' before end of module"));
                }
                return Ok(());
            }
            if nested && self.sym("}") {
                return Ok(());
            }
            if self.kw(Keyword::Use) {
                let start = self.tokens[self.at - 1].span.start;
                let target = self.path()?;
                let short = if self.kw(Keyword::As) {
                    self.name()?
                } else {
                    target
                        .rsplit("::")
                        .next()
                        .expect("nonempty path")
                        .to_owned()
                };
                self.expect(";")?;
                let span = start..self.tokens[self.at - 1].span.end;
                self.imports.push(Import {
                    module: self.module.clone(),
                    target,
                    name: self.qualify(&short),
                    span,
                });
                if self.imports.len() > 256 {
                    return Err(self.error("import limit exceeded (256)"));
                }
                continue;
            }
            if self.kw(Keyword::Mod) {
                if depth >= 16 {
                    return Err(self.error("module nesting limit exceeded (16)"));
                }
                let short = self.name()?;
                let name = self.qualify(&short);
                if !self.modules.insert(name.clone()) {
                    return Err(self.error("module already defined"));
                }
                if self.modules.len() > 128 {
                    return Err(self.error("module limit exceeded (128)"));
                }
                if self.token().kind == Kind::Sym(";") {
                    return Err(self.error(
                        "external modules require a project loader; use mod name { ... } for now",
                    ));
                }
                self.expect("{")?;
                let saved = std::mem::replace(&mut self.module, name);
                self.collect_items(true, depth + 1)?;
                self.module = saved;
                continue;
            }
            let start = self.at;
            let kind = if self.kw(Keyword::Fn) {
                Keyword::Fn
            } else if self.kw(Keyword::Struct) {
                Keyword::Struct
            } else {
                return Err(self.error("expected function definition, struct, mod or use"));
            };
            let name_at = self.at;
            let short = self.name()?;
            let name = self.qualify(&short);
            if kind == Keyword::Struct {
                if self
                    .pending_records
                    .insert(name.clone(), (name_at, self.module.clone()))
                    .is_some()
                {
                    return Err(self.error("struct already defined"));
                }
                if self.pending_records.len() > 128 {
                    return Err(self.error("struct limit exceeded (128)"));
                }
            } else {
                if !self.function_names.insert(name.clone()) {
                    return Err(self.error("function already defined"));
                }
                if self.function_names.len() > 256 {
                    return Err(self.error("function limit exceeded"));
                }
                while self.token().kind != Kind::Sym("{") {
                    if matches!(self.token().kind, Kind::Eof | Kind::Sym("}")) {
                        return Err(self.error("expected '{' for function body"));
                    }
                    self.at += 1;
                }
            }
            self.expect("{")?;
            let mut braces = 1usize;
            while braces != 0 {
                match self.token().kind {
                    Kind::Eof => return Err(self.error("expected '}' before end of file")),
                    Kind::Sym("{") => braces += 1,
                    Kind::Sym("}") => braces -= 1,
                    _ => {}
                }
                self.at += 1;
            }
            self.items.push(Item {
                kind,
                start,
                end: self.at,
                name,
                module: self.module.clone(),
            });
        }
    }
    fn program(mut self) -> Result<Program, Diagnostic> {
        self.collect_items(false, 0)?;
        self.resolve_imports()?;
        for item in self.items.clone() {
            if item.kind == Keyword::Struct {
                self.resolve_record(&item.name, self.tokens[item.start].span.clone())?;
            }
        }
        let mut functions: Vec<Function> = Vec::new();
        for item in self.items.clone() {
            if item.kind != Keyword::Fn {
                continue;
            }
            self.at = item.start;
            self.module = item.module;
            let start = self.token().span.start;
            self.kw(Keyword::Fn);
            self.name()?;
            let name = item.name;
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
            if self.at != item.end {
                return Err(self.error("unexpected tokens after function body"));
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
            Kind::Float(v) => {
                self.at += 1;
                ExprKind::Float(v)
            }
            Kind::Keyword(Keyword::True | Keyword::False) => {
                self.at += 1;
                ExprKind::Bool(token.kind == Kind::Keyword(Keyword::True))
            }
            Kind::Keyword(
                Keyword::I64
                | Keyword::U64
                | Keyword::I32
                | Keyword::U32
                | Keyword::I16
                | Keyword::U16
                | Keyword::I8
                | Keyword::U8
                | Keyword::F64
                | Keyword::Bool,
            ) => {
                let ty = self.type_name()?;
                self.expect("(")?;
                let value = self.expr(1)?;
                self.expect(")")?;
                ExprKind::Cast(ty, Box::new(value))
            }
            Kind::Ident | Kind::Sym("::") => {
                let name = self.path()?;
                let record = self
                    .lookup(&name, true)
                    .and_then(|name| self.records.get(&name).cloned());
                if let Some(record) = record
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
                    let target = self.lookup(&name, false).ok_or_else(|| {
                        Diagnostic::new(
                            self.source,
                            token.span.clone(),
                            format!("undefined function: {name}"),
                        )
                    })?;
                    ExprKind::Call(target, args)
                } else {
                    if name.contains("::") {
                        return Err(self.error(
                            "qualified names require a function call or struct initializer",
                        ));
                    }
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
