use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl SourceSpan {
    pub fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self {
            start,
            end: end.max(start),
            line: line.max(1),
            column: column.max(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    pub imports: Vec<Import>,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Import {
    pub module: String,
    pub items: Vec<String>, // Empty for "import module", non-empty for "from module import ..."
}

/// Type system for Cobble variables
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CobbleType {
    Integer, // i32 - stored in scoreboard
    Boolean, // bool - stored as 0/1 in scoreboard
    String,  // String - only in function params/macros
    List,    // List/Array - stored in storage
    Map,     // Dictionary/Object - stored in storage
    Unknown, // Type not yet inferred
}

impl CobbleType {
    /// Get human-readable type name
    pub fn name(&self) -> &str {
        match self {
            CobbleType::Integer => "integer",
            CobbleType::Boolean => "boolean",
            CobbleType::String => "string",
            CobbleType::List => "list",
            CobbleType::Map => "map",
            CobbleType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Statement {
    Import(Import),
    FunctionDef(FunctionDef),
    Assignment(Assignment),
    ConstAssignment(ConstAssignment), // const NAME = value
    Expression(Expression),
    If(IfStatement),
    For(ForLoop),
    While(WhileLoop),
    Match(MatchStatement), // match/switch statement
    Return(Option<Expression>),
    Pass,
    MinecraftCommand(String), // Commands starting with /
    Global(Vec<String>),      // global var1, var2, ...
    Execute(ExecuteBlock),    // as @s at @s: ... or asat @s: ...
    SelectorDef(SelectorDef), // @Name = @a[...]
    EntityDef(EntityDef),     // define @Name = @Selector ... create { ... }
    CreateEntity(String),     // create @Name
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityDef {
    pub name: String,
    pub selector: String,
    pub nbt: Expression,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub params: Vec<Parameter>,
    pub body: Vec<Statement>,
    pub decorators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub default: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub target: String,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstAssignment {
    pub target: String,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IfStatement {
    pub condition: Expression,
    pub then_branch: Vec<Statement>,
    pub elif_branches: Vec<(Expression, Vec<Statement>)>,
    pub else_branch: Option<Vec<Statement>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForLoop {
    pub target: String,
    pub iter: Expression,
    pub step: Option<Expression>, // Optional step value (for range with step)
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhileLoop {
    pub condition: Expression,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchStatement {
    pub value: Expression,
    pub cases: Vec<MatchCase>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchCase {
    pub pattern: MatchPattern,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MatchPattern {
    Literal(i32),    // case 5:
    Range(i32, i32), // case 1 to 10:
    Wildcard,        // case _:
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecuteBlock {
    pub modifiers: Vec<ExecuteModifier>, // as @a, at @s, if block ..., etc.
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecuteModifier {
    As(String),         // as @a
    At(String),         // at @s
    If(Expression),     // if x > 0 (Python-style expression)
    IfRaw(String),      // if block ~ ~ ~ stone (raw Minecraft syntax)
    Unless(Expression), // unless x > 0 (Python-style expression)
    UnlessRaw(String),  // unless entity @a[tag=done] (raw Minecraft syntax)
    Positioned(String), // positioned ~ ~1 ~
    Rotated(String),    // rotated ~ ~
    In(String),         // in minecraft:the_nether
    Anchored(String),   // anchored eyes
    Align(String),      // align xyz
    Store(String),      // store result score ...
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectorDef {
    pub name: String,     // e.g., "Player"
    pub selector: String, // e.g., "@a[type=player,gamemode=survival]"
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expression {
    // Literals
    Number(f64),
    String(String),
    Boolean(bool),
    None,
    Array(Vec<Expression>),
    Map(Vec<(String, Expression)>),

    // Identifiers and attributes
    Identifier(String),
    Attribute(Box<Expression>, String), // e.g., stdlib.event.TICK

    // Operations
    Binary(Box<Expression>, BinaryOp, Box<Expression>),
    Unary(UnaryOp, Box<Expression>),

    // Function calls
    Call(Box<Expression>, Vec<Expression>),

    // Subscript
    Subscript(Box<Expression>, Box<Expression>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BinaryOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,

    // Comparison
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,

    // Logical
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
    Pos,
}
