use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Statement {
    Import(Import),
    FunctionDef(FunctionDef),
    Class(ClassDef),
    Assignment(Assignment),
    Expression(Expression),
    If(IfStatement),
    For(ForLoop),
    While(WhileLoop),
    Return(Option<Expression>),
    Pass,
    MinecraftCommand(String), // Commands starting with /
    Global(Vec<String>),      // global var1, var2, ...
    Execute(ExecuteBlock),    // as @s at @s: ... or asat @s: ...
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
pub struct ClassDef {
    pub name: String,
    pub bases: Vec<String>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
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
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhileLoop {
    pub condition: Expression,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecuteBlock {
    pub modifiers: Vec<ExecuteModifier>, // as @a, at @s, if block ..., etc.
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecuteModifier {
    As(String),           // as @a
    At(String),           // at @s
    If(Expression),       // if x > 0 (Python-style expression)
    IfRaw(String),        // if block ~ ~ ~ stone (raw Minecraft syntax)
    Unless(Expression),   // unless x > 0 (Python-style expression)
    UnlessRaw(String),    // unless entity @a[tag=done] (raw Minecraft syntax)
    Positioned(String),   // positioned ~ ~1 ~
    Rotated(String),      // rotated ~ ~
    In(String),           // in minecraft:the_nether
    Anchored(String),     // anchored eyes
    Align(String),        // align xyz
    Store(String),        // store result score ...
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expression {
    // Literals
    Number(f64),
    String(String),
    Boolean(bool),
    None,

    // Identifiers and attributes
    Identifier(String),
    Attribute(Box<Expression>, String), // e.g., stdlib.event.TICK

    // Collections
    List(Vec<Expression>),
    Dict(Vec<(Expression, Expression)>),

    // Operations
    Binary(Box<Expression>, BinaryOp, Box<Expression>),
    Unary(UnaryOp, Box<Expression>),

    // Function calls
    Call(Box<Expression>, Vec<Expression>),

    // Subscript
    Subscript(Box<Expression>, Box<Expression>),

    // Lambda
    Lambda(Vec<String>, Box<Expression>),
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
    FloorDiv,

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

    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    LeftShift,
    RightShift,

    // Special
    In,
    NotIn,
    Is,
    IsNot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
    Pos,
    BitNot,
}
