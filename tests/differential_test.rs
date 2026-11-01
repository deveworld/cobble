//! Differential test: random programs run through the compiler and the
//! command simulator, and as Python, must end with the same module
//! variables, return value of `main`, and `say` output.
//!
//! Each program is generated once and printed twice. The Cobble text uses
//! the language's own operators; the Python text spells each operator as a
//! helper with the scoreboard's rules: 32-bit wrapping after every
//! operation, floor division and modulo, and `^` as a power whose negative
//! exponent gives 0. Divisors are never 0, loops are bounded, recursion
//! counts a parameter down to 0, and indexes stay inside their list or
//! string.
//!
//! Values are numbers, strings, and lists of numbers or strings. A program
//! is one file,
//! or a `main` file and one or two files it imports. A list is a value in
//! Cobble and an object in Python, so the generator never lets two names
//! reach the same list where a change through one could show through the
//! other: it doesn't copy list variables, pass module lists to functions,
//! change a list a function received, or assign a module list.
//!
//! `COBBLE_DIFF_CASES` sets the number of programs (default 300) and
//! `COBBLE_DIFF_SEED` the seed of the first one (default 1); program `i`
//! uses seed `COBBLE_DIFF_SEED + i`. The first failing program is shrunk
//! before it is printed; `COBBLE_DIFF_SHRINK=0` prints it as generated.
//! Without `python3` the test is skipped, unless
//! `COBBLE_DIFF_REQUIRE_PYTHON=1`, which CI sets.

mod common;

use common::mcsim::Sim;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write as _};
use std::panic::{self, AssertUnwindSafe};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// One source file and the module variables it defines.
struct ModuleSpec {
    name: &'static str,
    ints: &'static [&'static str],
    string: &'static str,
    list: &'static str,
    list_len: usize,
    /// A list of strings.
    strs: &'static str,
    strs_len: usize,
    /// A map whose fields `s` and `t` hold strings.
    rec: &'static str,
}

const MAIN: ModuleSpec = ModuleSpec {
    name: "main",
    ints: &["g0", "g1", "g2", "g3"],
    string: "gs",
    list: "gl",
    list_len: 3,
    strs: "gn",
    strs_len: 2,
    rec: "gr",
};
const UTIL: ModuleSpec = ModuleSpec {
    name: "util",
    ints: &["h0", "h1", "h2"],
    string: "hs",
    list: "hl",
    list_len: 2,
    strs: "hn",
    strs_len: 3,
    rec: "hr",
};
const EXTRA: ModuleSpec = ModuleSpec {
    name: "extra",
    ints: &["z0", "z1"],
    string: "zs",
    list: "zl",
    list_len: 3,
    strs: "zn",
    strs_len: 2,
    rec: "zr",
};

// Generated names never collide: locals and parameters are the names
// below, `w1`, `k2`, and `v3` are loop names, `f1` and `r2` are functions.
const INT_LOCALS: [&str; 4] = ["a", "b", "c", "d"];
const STR_LOCALS: [&str; 2] = ["s", "t"];
/// List locals and their lengths.
const LIST_LOCALS: [(&str, usize); 2] = [("xs", 3), ("ys", 2)];
/// Locals that hold a list of strings, and their lengths.
const STRS_LOCALS: [(&str, usize); 1] = [("ns", 2)];
/// The strings a program uses. None is empty, so `s[0]` and `s[-1]` exist.
const STRINGS: [&str; 6] = ["a", "ab", "xyz", "hello", "q r", "Mc"];
/// The fields of a map of strings.
const FIELDS: [&str; 2] = ["s", "t"];
/// The length of a list parameter.
const PARAM_LIST_LEN: usize = 2;

// ---------------------------------------------------------------------------
// Random source
// ---------------------------------------------------------------------------

/// SplitMix64, so a seed gives the same program on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + self.below((high - low + 1) as usize) as i32
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())].clone()
    }
}

// ---------------------------------------------------------------------------
// Program model
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

impl Op {
    fn symbol(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Mod => "%",
            Op::Pow => "^",
        }
    }

    fn helper(self) -> &'static str {
        match self {
            Op::Add => "add",
            Op::Sub => "sub",
            Op::Mul => "mul",
            Op::Div => "fdiv",
            Op::Mod => "fmod",
            Op::Pow => "pw",
        }
    }

    fn precedence(self) -> u8 {
        match self {
            Op::Add | Op::Sub => 2,
            Op::Mul | Op::Div | Op::Mod => 3,
            Op::Pow => 4,
        }
    }
}

/// The type of a variable. A list's length never changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Int,
    Str,
    List(usize),
    /// A list of strings.
    Strs(usize),
    /// A map whose fields `s` and `t` hold strings.
    Rec,
}

#[derive(Clone)]
enum Expr {
    Lit(i32),
    Var(String),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(String, Vec<Val>),
    /// `list[index]`.
    Item(String, Box<Expr>),
    /// `len(name)` of a list or a string.
    Len(String),
    /// `(value if condition else otherwise)`.
    Pick(Box<Cond>, Box<Expr>, Box<Expr>),
    Abs(Box<Expr>),
    /// `min(a, b)`, or `max(a, b)` when the flag is set.
    Extreme(bool, Box<Expr>, Box<Expr>),
    /// `(a or b)`, or `(a and b)` when the flag is clear.
    Either(bool, Box<Expr>, Box<Expr>),
    /// `sum(name)` of a list of numbers.
    Sum(String),
    /// `min(name)`, or `max(name)` when the flag is set.
    ListExtreme(bool, String),
    /// `name.count(value)`.
    Count(String, Box<Expr>),
    /// `len(text)` of any string value.
    StrLen(Box<Str>),
}

fn bin(op: Op, left: Expr, right: Expr) -> Expr {
    Expr::Bin(op, Box::new(left), Box::new(right))
}

#[derive(Clone)]
enum Str {
    Lit(&'static str),
    Var(String),
    /// `name[index]`, a one-character string.
    Char(String, Box<Expr>),
    /// `name[index]` of a list of strings.
    Item(String, Box<Expr>),
    /// `name[index][at]`: a character of an item of a list of strings.
    ItemChar(String, Box<Expr>, Box<Expr>),
    /// `name.field`, or `name["field"]` when the flag is set.
    Field(String, &'static str, bool),
    /// `name.field[at]`: a character of a field.
    FieldChar(String, &'static str, bool, Box<Expr>),
    /// `(left + right)`.
    Join(Box<Str>, Box<Str>),
    /// `(text * count)` with a count of 1 or more.
    Repeat(Box<Str>, Box<Expr>),
}

/// Where a string statement writes: a variable, an item of a list of
/// strings, or a field of a map of strings.
#[derive(Clone)]
enum Place {
    Var(String),
    Item(String, Expr),
    Field(String, &'static str, bool),
}

#[derive(Clone)]
enum List {
    Lit(Vec<Expr>),
    Var(String),
}

/// A list of strings.
#[derive(Clone)]
enum Strs {
    Lit(Vec<Str>),
    Var(String),
}

#[derive(Clone)]
enum Val {
    Int(Expr),
    Str(Str),
    List(List),
    Strs(Strs),
    /// `{"s": s, "t": t}`.
    Rec(Str, Str),
}

#[derive(Clone)]
enum Cond {
    Cmp(&'static str, Expr, Expr),
    Truthy(Expr),
    /// `bool` asks for parentheses around the operand when they aren't
    /// needed, so both spellings of `not a < b` get compiled.
    Not(Box<Cond>, bool),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    /// `value in name`, or `value not in name` when the flag is set.
    In(Box<Expr>, String, bool),
    /// `left == right` or `left != right` on strings.
    Text(&'static str, Str, Str),
    /// `left <op> middle <op> right`.
    Chain(Box<Expr>, &'static str, Box<Expr>, &'static str, Box<Expr>),
    /// `text`, true when it isn't empty, or `not text` when the flag is set.
    TextTruth(Str, bool),
}

#[derive(Clone)]
enum Stmt {
    Assign(String, Val),
    Compound(String, Op, Expr),
    /// `list[index] = value`.
    SetItem(String, Expr, Expr),
    /// `strings[index] = value` on a list of strings.
    SetStr(String, Expr, Str),
    Call(String, Vec<Val>),
    /// `/scoreboard players add|remove|set <name> temp <value>`.
    Raw(&'static str, String, i32),
    Say(Vec<String>),
    If(Vec<(Cond, Vec<Stmt>)>, Option<Vec<Stmt>>),
    /// `counter = 0`, then `while counter < limit [and extra]:` with
    /// `counter += 1` at the end of the body.
    While {
        counter: String,
        limit: Expr,
        extra: Option<Cond>,
        body: Vec<Stmt>,
    },
    For {
        target: String,
        args: Vec<Expr>,
        body: Vec<Stmt>,
    },
    /// `for target in list:`, over a list of numbers or of strings.
    ForEach {
        target: String,
        list: Val,
        body: Vec<Stmt>,
    },
    Return(Expr),
    /// `a, b = b, a`.
    Swap(String, String),
    /// `list.append(value)`.
    Append(String, Expr),
    /// `place = text` on a field.
    SetField(String, &'static str, bool, Str),
    /// `place += text`.
    JoinTo(Place, Str),
    /// `place *= count`.
    RepeatAt(Place, Expr),
}

#[derive(Clone)]
struct Function {
    name: String,
    params: Vec<String>,
    globals: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Clone)]
enum Import {
    /// `import module`; calls spell `module.f`.
    Module(&'static str),
    /// `from module import f, g`.
    Names(&'static str, Vec<String>),
}

#[derive(Clone)]
struct Module {
    name: &'static str,
    imports: Vec<Import>,
    vars: Vec<(String, Val)>,
    /// Module-level code after the variables, such as a loop over the
    /// module's list, which runs when the module loads.
    code: Vec<Stmt>,
    functions: Vec<Function>,
    /// `const` declarations after the functions, which the functions read.
    consts: Vec<(String, i32)>,
}

/// The modules in the order Python runs them: each after the modules it
/// imports, `main` last.
#[derive(Clone)]
struct Program {
    modules: Vec<Module>,
}

// ---------------------------------------------------------------------------
// Generator
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Callee {
    /// How the caller spells the function: `f2`, or `util.f2`.
    name: String,
    params: Vec<Ty>,
    /// Recursive callees take a depth first, which must stay small.
    recursive: bool,
}

/// What a function body may name while it is generated.
#[derive(Clone)]
struct Scope {
    params: Vec<(String, Ty)>,
    /// Parameters the body must not assign, such as a recursion depth.
    fixed_params: Vec<String>,
    globals: Vec<String>,
    /// The variables of the function's module.
    module_vars: Vec<(String, Ty)>,
    /// The module's constants, declared below the functions: readable
    /// numbers, never assigned.
    consts: Vec<String>,
    /// Names the body may assign as locals, including module variable names
    /// it shadows.
    local_pool: Vec<(String, Ty)>,
    /// Locals assigned on every path to the current statement.
    assigned: BTreeSet<String>,
    /// Loop counters and targets: readable, never assigned by the body.
    loop_names: Vec<(String, Ty)>,
    /// Lists that a loop around the current statement iterates. Python
    /// keeps iterating the old list after an assignment, so the body
    /// doesn't assign them.
    iterated: Vec<String>,
    callees: Vec<Callee>,
    /// The calls left for this function; bounds the work a program does.
    calls_left: usize,
    loops: usize,
    /// Whether `*=` may repeat text: only in `main` outside loops, so text
    /// stays short.
    repeats: bool,
}

impl Scope {
    fn hidden(&self, name: &str) -> bool {
        self.params.iter().any(|(p, _)| p == name) || self.local_pool.iter().any(|(l, _)| l == name)
    }

    fn readable(&self, ty: Ty) -> Vec<String> {
        let mut names: Vec<String> = self
            .params
            .iter()
            .filter(|(_, t)| *t == ty)
            .map(|(p, _)| p.clone())
            .collect();
        names.extend(
            self.local_pool
                .iter()
                .filter(|(name, t)| *t == ty && self.assigned.contains(name))
                .map(|(name, _)| name.clone()),
        );
        names.extend(
            self.loop_names
                .iter()
                .filter(|(_, t)| *t == ty)
                .map(|(name, _)| name.clone()),
        );
        for (var, t) in &self.module_vars {
            if *t == ty && !self.hidden(var) {
                names.push(var.clone());
            }
        }
        if ty == Ty::Int {
            names.extend(self.consts.iter().cloned());
        }
        names.sort();
        names.dedup();
        names
    }

    /// Readable lists of numbers of any length.
    fn readable_lists(&self) -> Vec<(String, usize)> {
        let mut lists = Vec::new();
        for len in [2, 3] {
            lists.extend(self.readable(Ty::List(len)).into_iter().map(|n| (n, len)));
        }
        lists
    }

    /// Readable lists of strings of any length.
    fn readable_strs(&self) -> Vec<(String, usize)> {
        let mut lists = Vec::new();
        for len in [2, 3] {
            lists.extend(self.readable(Ty::Strs(len)).into_iter().map(|n| (n, len)));
        }
        lists
    }

    fn writable(&self, ty: Ty) -> Vec<String> {
        let mut names: Vec<String> = self
            .params
            .iter()
            .filter(|(p, t)| *t == ty && !self.fixed_params.contains(p))
            .map(|(p, _)| p.clone())
            .collect();
        // Module lists are never assigned; see the module docs.
        if !matches!(ty, Ty::List(_) | Ty::Strs(_)) {
            names.extend(
                self.globals
                    .iter()
                    .filter(|g| self.module_vars.iter().any(|(v, t)| v == *g && *t == ty))
                    .cloned(),
            );
        }
        names.extend(
            self.local_pool
                .iter()
                .filter(|(_, t)| *t == ty)
                .map(|(name, _)| name.clone()),
        );
        // A loop variable isn't assigned by its loop's body. A body may
        // rebind a list of its own that a loop walks: Python keeps walking
        // the old list. A module list is never assigned.
        names.retain(|name| {
            !self
                .loop_names
                .iter()
                .any(|(loop_name, _)| loop_name == name)
                && (!self.iterated.contains(name) || matches!(ty, Ty::List(_) | Ty::Strs(_)))
        });
        names
    }

    /// Lists whose items a statement may set: the function's own lists and
    /// module lists. A list the function received is not, since Python
    /// would change the caller's list.
    fn item_targets(&self) -> Vec<(String, usize)> {
        self.readable_lists()
            .into_iter()
            .filter(|(name, _)| !self.params.iter().any(|(p, _)| p == name))
            .collect()
    }

    /// Lists of strings whose items a statement may set. No parameter holds
    /// one.
    fn str_item_targets(&self) -> Vec<(String, usize)> {
        self.readable_strs()
    }

    /// Lists that may be passed to a function: the function's own lists and
    /// the lists it received. A module list could change during the call.
    fn passable_lists(&self, len: usize) -> Vec<String> {
        self.readable(Ty::List(len))
            .into_iter()
            .filter(|name| !self.module_vars.iter().any(|(v, _)| v == name) || self.hidden(name))
            .collect()
    }

    /// Score-backed names a raw scoreboard command may change: assigned
    /// number locals and declared number globals. Parameters live in
    /// storage.
    fn raw_targets(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .local_pool
            .iter()
            .filter(|(name, t)| *t == Ty::Int && self.assigned.contains(name))
            .map(|(name, _)| name.clone())
            .collect();
        names.extend(
            self.globals
                .iter()
                .filter(|g| {
                    self.module_vars
                        .iter()
                        .any(|(v, t)| v == *g && *t == Ty::Int)
                })
                .cloned(),
        );
        names
    }

    /// Whether `name` is a local that some path to here leaves unassigned.
    fn is_unassigned_local(&self, name: &str) -> bool {
        self.local_pool.iter().any(|(l, _)| l == name) && !self.assigned.contains(name)
    }
}

struct Generator {
    rng: Rng,
    counter: usize,
    functions: usize,
    /// The constants of the module being generated.
    consts: Vec<String>,
}

impl Generator {
    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn literal(&mut self) -> i32 {
        match self.rng.below(20) {
            0 => self.rng.pick(&[i32::MAX, -i32::MAX, 65536, -65536, 100000]),
            1..=3 => self.rng.range(-1000, 1000),
            _ => self.rng.range(-9, 9),
        }
    }

    fn atom(&mut self, scope: &mut Scope, depth: usize) -> Expr {
        if depth > 0 && scope.calls_left > 0 && !scope.callees.is_empty() && self.rng.chance(15) {
            return self.call_expr(scope, depth);
        }
        if depth > 0 && self.rng.chance(12) {
            let lists = scope.readable_lists();
            let strings = scope.readable(Ty::Str);
            let strs = scope.readable_strs();
            match self.rng.below(4) {
                0 if !lists.is_empty() => {
                    let (name, len) = self.rng.pick(&lists);
                    let index = self.list_index(scope, len, depth - 1);
                    return Expr::Item(name, Box::new(index));
                }
                1 if !lists.is_empty() => return Expr::Len(self.rng.pick(&lists).0),
                2 if !strings.is_empty() => return Expr::Len(self.rng.pick(&strings)),
                3 if !strs.is_empty() => return Expr::Len(self.rng.pick(&strs).0),
                _ => {}
            }
        }
        if depth > 0 && self.rng.chance(4) {
            return Expr::StrLen(Box::new(self.str_expr(scope, depth - 1)));
        }
        let names = scope.readable(Ty::Int);
        if !names.is_empty() && self.rng.chance(60) {
            return Expr::Var(self.rng.pick(&names));
        }
        Expr::Lit(self.literal())
    }

    fn call_expr(&mut self, scope: &mut Scope, depth: usize) -> Expr {
        scope.calls_left -= 1;
        let callee = self.rng.pick(&scope.callees);
        let mut args = Vec::new();
        for (index, ty) in callee.params.iter().enumerate() {
            let arg = self.val(scope, *ty, depth.saturating_sub(1), true);
            args.push(match arg {
                Val::Int(arg) if callee.recursive && index == 0 => {
                    Val::Int(bin(Op::Mod, arg, Expr::Lit(4)))
                }
                arg => arg,
            });
        }
        Expr::Call(callee.name, args)
    }

    /// A divisor that can't be 0: a non-zero literal, or
    /// `e % 5 + 6` (6..10) or `e % -5 - 6` (-10..-6).
    fn divisor(&mut self, scope: &mut Scope, depth: usize) -> Expr {
        if self.rng.chance(50) {
            let value = self.rng.pick(&[1, 2, 3, 7, -1, -2, -3, -7, 1000, -65536]);
            return Expr::Lit(value);
        }
        let inner = self.expr(scope, depth);
        if self.rng.chance(50) {
            bin(Op::Add, bin(Op::Mod, inner, Expr::Lit(5)), Expr::Lit(6))
        } else {
            bin(Op::Sub, bin(Op::Mod, inner, Expr::Lit(-5)), Expr::Lit(6))
        }
    }

    /// A small exponent: a literal 0..4 or `e % 5 - 1` (-1..3).
    fn exponent(&mut self, scope: &mut Scope, depth: usize) -> Expr {
        if self.rng.chance(60) {
            return Expr::Lit(self.rng.range(0, 4));
        }
        let inner = self.expr(scope, depth);
        bin(Op::Sub, bin(Op::Mod, inner, Expr::Lit(5)), Expr::Lit(1))
    }

    /// An index into a list of `len` items, counted from either end.
    fn list_index(&mut self, scope: &mut Scope, len: usize, depth: usize) -> Expr {
        let len = len as i32;
        match self.rng.below(4) {
            0 => Expr::Lit(self.rng.range(-len, len - 1)),
            1 | 2 => bin(Op::Mod, self.expr(scope, depth), Expr::Lit(len)),
            _ => bin(
                Op::Sub,
                bin(Op::Mod, self.expr(scope, depth), Expr::Lit(len)),
                Expr::Lit(len),
            ),
        }
    }

    /// An index into the string `name`: its first or last character, or
    /// `e % len(name)`.
    fn string_index(&mut self, scope: &mut Scope, name: &str, depth: usize) -> Expr {
        match self.rng.below(3) {
            0 => Expr::Lit(0),
            1 => Expr::Lit(-1),
            _ => bin(
                Op::Mod,
                self.expr(scope, depth),
                Expr::Len(name.to_string()),
            ),
        }
    }

    fn expr(&mut self, scope: &mut Scope, depth: usize) -> Expr {
        if depth == 0 || self.rng.chance(30) {
            return self.atom(scope, depth);
        }
        if self.rng.chance(16) {
            let lists = scope.readable_lists();
            let left = Box::new(self.expr(scope, depth - 1));
            let right = Box::new(self.expr(scope, depth - 1));
            match self.rng.below(7) {
                0 => return Expr::Pick(Box::new(self.cond(scope, 1)), left, right),
                1 => return Expr::Abs(left),
                2 => return Expr::Extreme(self.rng.chance(50), left, right),
                3 => return Expr::Either(self.rng.chance(50), left, right),
                4 if !lists.is_empty() => return Expr::Sum(self.rng.pick(&lists).0),
                5 if !lists.is_empty() => {
                    let flag = self.rng.chance(50);
                    return Expr::ListExtreme(flag, self.rng.pick(&lists).0);
                }
                6 if !lists.is_empty() => {
                    return Expr::Count(self.rng.pick(&lists).0, left);
                }
                _ => {}
            }
        }
        if self.rng.chance(6) {
            return Expr::Neg(Box::new(self.expr(scope, depth - 1)));
        }
        let op = match self.rng.below(20) {
            0..=5 => Op::Add,
            6..=9 => Op::Sub,
            10..=13 => Op::Mul,
            14..=15 => Op::Div,
            16..=17 => Op::Mod,
            _ => Op::Pow,
        };
        let left = self.expr(scope, depth - 1);
        let right = match op {
            Op::Div | Op::Mod => self.divisor(scope, depth - 1),
            Op::Pow => self.exponent(scope, depth - 1),
            _ => self.expr(scope, depth - 1),
        };
        bin(op, left, right)
    }

    /// An index into the text `base`: its first or last character, or
    /// `e % len(base)`.
    fn char_index(&mut self, scope: &mut Scope, base: Str, depth: usize) -> Expr {
        match self.rng.below(3) {
            0 => Expr::Lit(0),
            1 => Expr::Lit(-1),
            _ => bin(
                Op::Mod,
                self.expr(scope, depth),
                Expr::StrLen(Box::new(base)),
            ),
        }
    }

    /// A string that stays short: a literal or a character.
    fn short_str(&mut self, scope: &mut Scope) -> Str {
        match self.str_expr(scope, 0) {
            text @ (Str::Lit(_) | Str::Char(..) | Str::ItemChar(..) | Str::FieldChar(..)) => text,
            _ => Str::Lit(self.rng.pick(&STRINGS)),
        }
    }

    /// Text in a container, or text built from others: a field, a
    /// character of an item or a field, a join, or a repeat. `None` when
    /// the scope has no container to read.
    fn container_str(&mut self, scope: &mut Scope, depth: usize) -> Option<Str> {
        let recs = scope.readable(Ty::Rec);
        let strs = scope.readable_strs();
        match self.rng.below(5) {
            0 if !recs.is_empty() => {
                let field = self.rng.pick(&FIELDS);
                Some(Str::Field(self.rng.pick(&recs), field, self.rng.chance(30)))
            }
            1 if !recs.is_empty() => {
                let (name, field, key) = (
                    self.rng.pick(&recs),
                    self.rng.pick(&FIELDS),
                    self.rng.chance(30),
                );
                let at = self.char_index(scope, Str::Field(name.clone(), field, key), 0);
                Some(Str::FieldChar(name, field, key, Box::new(at)))
            }
            2 if !strs.is_empty() => {
                let (name, len) = self.rng.pick(&strs);
                let index = self.list_index(scope, len, 0);
                let item = Str::Item(name.clone(), Box::new(index.clone()));
                let at = self.char_index(scope, item, 0);
                Some(Str::ItemChar(name, Box::new(index), Box::new(at)))
            }
            3 if depth > 0 => {
                let short = self.short_str(scope);
                let other = self.str_expr(scope, depth - 1);
                Some(if self.rng.chance(50) {
                    Str::Join(Box::new(short), Box::new(other))
                } else {
                    Str::Join(Box::new(other), Box::new(short))
                })
            }
            4 if depth > 0 => {
                let count = match self.rng.below(3) {
                    0 => bin(
                        Op::Add,
                        bin(Op::Mod, self.expr(scope, 0), Expr::Lit(2)),
                        Expr::Lit(1),
                    ),
                    _ => Expr::Lit(self.rng.range(1, 3)),
                };
                Some(Str::Repeat(
                    Box::new(self.short_str(scope)),
                    Box::new(count),
                ))
            }
            _ => None,
        }
    }

    fn str_expr(&mut self, scope: &mut Scope, depth: usize) -> Str {
        let names = scope.readable(Ty::Str);
        let strs = scope.readable_strs();
        if self.rng.chance(30) {
            if let Some(text) = self.container_str(scope, depth) {
                return text;
            }
        }
        match self.rng.below(12) {
            4..=6 if !names.is_empty() => Str::Var(self.rng.pick(&names)),
            7..=9 if !names.is_empty() => {
                let name = self.rng.pick(&names);
                let index = self.string_index(scope, &name, depth.min(1));
                Str::Char(name, Box::new(index))
            }
            10..=11 if !strs.is_empty() => {
                let (name, len) = self.rng.pick(&strs);
                let index = self.list_index(scope, len, depth.min(1));
                Str::Item(name, Box::new(index))
            }
            _ => Str::Lit(self.rng.pick(&STRINGS)),
        }
    }

    /// A value of type `ty`. An argument may also pass a list variable.
    fn val(&mut self, scope: &mut Scope, ty: Ty, depth: usize, argument: bool) -> Val {
        match ty {
            Ty::Rec => Val::Rec(self.str_expr(scope, 1), self.str_expr(scope, 1)),
            Ty::Int => Val::Int(self.expr(scope, depth)),
            Ty::Str => Val::Str(self.str_expr(scope, depth)),
            Ty::Strs(len) => {
                let items = (0..len)
                    .map(|_| self.str_expr(scope, depth.min(1)))
                    .collect();
                Val::Strs(Strs::Lit(items))
            }
            Ty::List(len) => {
                let names = scope.passable_lists(len);
                if argument && !names.is_empty() && self.rng.chance(50) {
                    return Val::List(List::Var(self.rng.pick(&names)));
                }
                let items = (0..len)
                    .map(|_| {
                        if self.rng.chance(50) {
                            Expr::Lit(self.literal())
                        } else {
                            self.expr(scope, depth.min(2))
                        }
                    })
                    .collect();
                Val::List(List::Lit(items))
            }
        }
    }

    fn cond(&mut self, scope: &mut Scope, depth: usize) -> Cond {
        if depth > 0 && self.rng.chance(35) {
            return match self.rng.below(3) {
                0 => Cond::Not(Box::new(self.cond(scope, depth - 1)), self.rng.chance(50)),
                1 => Cond::And(
                    Box::new(self.cond(scope, depth - 1)),
                    Box::new(self.cond(scope, depth - 1)),
                ),
                _ => Cond::Or(
                    Box::new(self.cond(scope, depth - 1)),
                    Box::new(self.cond(scope, depth - 1)),
                ),
            };
        }
        if self.rng.chance(15) {
            return Cond::Truthy(self.expr(scope, 1));
        }
        if self.rng.chance(5) {
            return Cond::TextTruth(self.str_expr(scope, 1), self.rng.chance(40));
        }
        if self.rng.chance(20) {
            let lists = scope.readable_lists();
            match self.rng.below(3) {
                0 if !lists.is_empty() => {
                    let value = Box::new(self.expr(scope, 1));
                    return Cond::In(value, self.rng.pick(&lists).0, self.rng.chance(40));
                }
                1 => {
                    let op = self.rng.pick(&["==", "!="]);
                    return Cond::Text(op, self.str_expr(scope, 1), self.str_expr(scope, 1));
                }
                _ => {
                    let first = self.rng.pick(&["<", "<=", "=="]);
                    let second = self.rng.pick(&["<", "<=", "!="]);
                    let names = scope.readable(Ty::Int);
                    let middle = if names.is_empty() {
                        Expr::Lit(self.literal())
                    } else {
                        Expr::Var(self.rng.pick(&names))
                    };
                    let left = Box::new(self.expr(scope, 1));
                    let right = Box::new(self.expr(scope, 1));
                    return Cond::Chain(left, first, Box::new(middle), second, right);
                }
            }
        }
        let op = self.rng.pick(&["==", "!=", "<", "<=", ">", ">="]);
        Cond::Cmp(op, self.expr(scope, 2), self.expr(scope, 2))
    }

    /// Statements for one block. `depth` limits nesting; a block ends after
    /// a `return`.
    fn block(&mut self, scope: &mut Scope, depth: usize, length: usize) -> Vec<Stmt> {
        let mut body = Vec::new();
        for _ in 0..length.max(1) {
            let statement = self.statement(scope, depth);
            let returns = matches!(statement, Stmt::Return(_));
            body.push(statement);
            if returns {
                break;
            }
        }
        body
    }

    fn statement(&mut self, scope: &mut Scope, depth: usize) -> Stmt {
        loop {
            match self.rng.below(100) {
                0..=29 => {
                    let ty = match self.rng.below(12) {
                        0 | 1 => Ty::Str,
                        2 => self.rng.pick(&[Ty::List(2), Ty::List(3)]),
                        3 => self.rng.pick(&[Ty::Strs(2), Ty::Strs(3)]),
                        _ => Ty::Int,
                    };
                    let targets = scope.writable(ty);
                    if targets.is_empty() {
                        continue;
                    }
                    let target = self.rng.pick(&targets);
                    let value = self.val(scope, ty, 3, false);
                    scope.assigned.insert(target.clone());
                    return Stmt::Assign(target, value);
                }
                30..=39 => {
                    let targets: Vec<String> = scope
                        .writable(Ty::Int)
                        .into_iter()
                        .filter(|name| !scope.is_unassigned_local(name))
                        .collect();
                    if targets.is_empty() {
                        continue;
                    }
                    let target = self.rng.pick(&targets);
                    let op = self
                        .rng
                        .pick(&[Op::Add, Op::Sub, Op::Mul, Op::Div, Op::Mod, Op::Pow]);
                    let value = match op {
                        Op::Div | Op::Mod => self.divisor(scope, 1),
                        Op::Pow => self.exponent(scope, 1),
                        _ => self.expr(scope, 2),
                    };
                    return Stmt::Compound(target, op, value);
                }
                40..=43 => {
                    let lists = scope.item_targets();
                    let strs = scope.str_item_targets();
                    if !strs.is_empty() && (lists.is_empty() || self.rng.chance(30)) {
                        let (name, len) = self.rng.pick(&strs);
                        let index = self.list_index(scope, len, 1);
                        let value = self.str_expr(scope, 1);
                        return Stmt::SetStr(name, index, value);
                    }
                    if lists.is_empty() {
                        continue;
                    }
                    let (name, len) = self.rng.pick(&lists);
                    let index = self.list_index(scope, len, 1);
                    let value = self.expr(scope, 2);
                    return Stmt::SetItem(name, index, value);
                }
                44..=49 => {
                    if scope.calls_left == 0 || scope.callees.is_empty() {
                        continue;
                    }
                    let Expr::Call(name, args) = self.call_expr(scope, 2) else {
                        unreachable!()
                    };
                    return Stmt::Call(name, args);
                }
                50..=54 => {
                    let targets = scope.raw_targets();
                    if targets.is_empty() {
                        continue;
                    }
                    let action = self.rng.pick(&["add", "remove", "set"]);
                    return Stmt::Raw(action, self.rng.pick(&targets), self.rng.range(0, 50));
                }
                55..=59 => {
                    let mut names = scope.readable(Ty::Int);
                    names.extend(scope.readable(Ty::Str));
                    if names.is_empty() {
                        continue;
                    }
                    let count = self.rng.range(1, 2) as usize;
                    return Stmt::Say((0..count).map(|_| self.rng.pick(&names)).collect());
                }
                60..=73 if depth > 0 => return self.if_statement(scope, depth),
                74..=79 if depth > 0 && scope.loops < 2 => {
                    return self.while_statement(scope, depth)
                }
                80..=84 if depth > 0 && scope.loops < 2 => return self.for_statement(scope, depth),
                85..=88 if depth > 0 && scope.loops < 2 => {
                    return self.foreach_statement(scope, depth)
                }
                89..=92 if depth < 2 => return Stmt::Return(self.expr(scope, 2)),
                93 | 94 => {
                    let targets: Vec<String> = scope
                        .writable(Ty::Int)
                        .into_iter()
                        .filter(|name| !scope.is_unassigned_local(name))
                        .collect();
                    if targets.len() < 2 {
                        continue;
                    }
                    let first = self.rng.pick(&targets);
                    let second = self.rng.pick(&targets);
                    if first == second {
                        continue;
                    }
                    return Stmt::Swap(first, second);
                }
                95 | 96 => {
                    // A local list that no loop walks: a loop over a list
                    // that grows would not end in Python.
                    let lists: Vec<(String, usize)> = scope
                        .item_targets()
                        .into_iter()
                        .filter(|(name, _)| {
                            !scope.iterated.contains(name)
                                && scope.local_pool.iter().any(|(local, _)| local == name)
                        })
                        .collect();
                    if lists.is_empty() {
                        continue;
                    }
                    let name = self.rng.pick(&lists).0;
                    return Stmt::Append(name, self.expr(scope, 2));
                }
                97..=99 => {
                    if let Some(statement) = self.text_statement(scope) {
                        return statement;
                    }
                }
                _ => {}
            }
        }
    }

    /// A write of text into a field, or `+=` or `*=` on text in a
    /// variable, an item, or a field.
    fn text_statement(&mut self, scope: &mut Scope) -> Option<Stmt> {
        let mut places = Vec::new();
        for name in scope.writable(Ty::Str) {
            if !scope.is_unassigned_local(&name) {
                places.push(Place::Var(name));
            }
        }
        for (name, len) in scope.str_item_targets() {
            let index = self.list_index(scope, len, 1);
            places.push(Place::Item(name, index));
        }
        for name in scope.readable(Ty::Rec) {
            let field = self.rng.pick(&FIELDS);
            places.push(Place::Field(name, field, self.rng.chance(30)));
        }
        if places.is_empty() {
            return None;
        }
        let place = self.rng.pick(&places);
        match self.rng.below(3) {
            0 => {
                let Place::Field(name, field, key) = place else {
                    return None;
                };
                let value = self.str_expr(scope, 2);
                Some(Stmt::SetField(name, field, key, value))
            }
            1 => {
                let value = self.short_str(scope);
                Some(Stmt::JoinTo(place, value))
            }
            _ if scope.repeats && scope.loops == 0 => {
                let count = Expr::Lit(self.rng.range(1, 2));
                Some(Stmt::RepeatAt(place, count))
            }
            _ => None,
        }
    }

    fn if_statement(&mut self, scope: &mut Scope, depth: usize) -> Stmt {
        let before = scope.assigned.clone();
        let mut after: Option<BTreeSet<String>> = None;
        let mut branches = Vec::new();
        let mut merge = |scope: &mut Scope, returns: bool| {
            // A branch that returns doesn't reach the code after the `if`.
            if !returns {
                after = Some(match after.take() {
                    Some(set) => set.intersection(&scope.assigned).cloned().collect(),
                    None => scope.assigned.clone(),
                });
            }
            scope.assigned = before.clone();
        };
        for _ in 0..self.rng.range(1, 3) {
            let condition = self.cond(scope, 2);
            let length = self.rng.range(1, 3) as usize;
            let body = self.block(scope, depth - 1, length);
            let returns = matches!(body.last(), Some(Stmt::Return(_)));
            merge(scope, returns);
            branches.push((condition, body));
        }
        let else_branch = if self.rng.chance(50) {
            let length = self.rng.range(1, 3) as usize;
            let body = self.block(scope, depth - 1, length);
            let returns = matches!(body.last(), Some(Stmt::Return(_)));
            merge(scope, returns);
            Some(body)
        } else {
            // No `else`: the code after the `if` may run with no branch taken.
            merge(scope, false);
            None
        };
        scope.assigned = after.unwrap_or(before);
        Stmt::If(branches, else_branch)
    }

    fn loop_body(&mut self, scope: &mut Scope, depth: usize, loop_name: &str, ty: Ty) -> Vec<Stmt> {
        let before = scope.assigned.clone();
        scope.loops += 1;
        scope.loop_names.push((loop_name.to_string(), ty));
        // A call in a loop runs on every pass, so a loop body gets at most
        // one of the function's calls.
        let calls_left = scope.calls_left;
        let budget = calls_left.min(1);
        scope.calls_left = budget;
        let length = self.rng.range(1, 3) as usize;
        let body = self.block(scope, depth - 1, length);
        scope.calls_left = calls_left - (budget - scope.calls_left);
        scope.loop_names.pop();
        scope.loops -= 1;
        // The body may not run at all.
        scope.assigned = before;
        body
    }

    fn while_statement(&mut self, scope: &mut Scope, depth: usize) -> Stmt {
        let counter = self.fresh("w");
        let limit = if self.rng.chance(50) {
            Expr::Lit(self.rng.range(0, 3))
        } else {
            bin(Op::Mod, self.expr(scope, 2), Expr::Lit(4))
        };
        // `limit` is evaluated before each pass, so it may only read names
        // the body can't change.
        let limit = if Self::reads_writable(&limit, scope) {
            Expr::Lit(self.rng.range(0, 3))
        } else {
            limit
        };
        let extra = self.rng.chance(30).then(|| {
            let calls_left = scope.calls_left;
            scope.calls_left = 0;
            let extra = self.cond(scope, 1);
            scope.calls_left = calls_left;
            extra
        });
        let body = self.loop_body(scope, depth, &counter, Ty::Int);
        Stmt::While {
            counter,
            limit,
            extra,
            body,
        }
    }

    fn reads_writable(expr: &Expr, scope: &Scope) -> bool {
        match expr {
            Expr::Lit(_) => false,
            Expr::Var(name) => scope.writable(Ty::Int).contains(name),
            Expr::Neg(inner) => Self::reads_writable(inner, scope),
            Expr::Bin(_, left, right) => {
                Self::reads_writable(left, scope) || Self::reads_writable(right, scope)
            }
            // A list's items and a string can change in the body.
            _ => true,
        }
    }

    fn for_statement(&mut self, scope: &mut Scope, depth: usize) -> Stmt {
        let target = self.fresh("k");
        let bound = |generator: &mut Self, scope: &mut Scope| {
            if generator.rng.chance(50) {
                Expr::Lit(generator.rng.range(-2, 4))
            } else {
                bin(Op::Mod, generator.expr(scope, 2), Expr::Lit(4))
            }
        };
        let args = match self.rng.below(3) {
            0 => vec![bound(self, scope)],
            1 => vec![bound(self, scope), bound(self, scope)],
            _ => vec![
                bound(self, scope),
                bound(self, scope),
                Expr::Lit(self.rng.pick(&[1, 2, -1, -2])),
            ],
        };
        let body = self.loop_body(scope, depth, &target, Ty::Int);
        Stmt::For { target, args, body }
    }

    /// A `for` loop over a list of numbers or of strings.
    fn foreach_statement(&mut self, scope: &mut Scope, depth: usize) -> Stmt {
        let target = self.fresh("v");
        let strings = self.rng.chance(35);
        let names: Vec<String> = if strings {
            scope.readable_strs().into_iter().map(|(n, _)| n).collect()
        } else {
            scope.readable_lists().into_iter().map(|(n, _)| n).collect()
        };
        let length = self.rng.range(1, 3);
        let (list, ty) = match (strings, !names.is_empty() && self.rng.chance(60)) {
            (true, true) => (Val::Strs(Strs::Var(self.rng.pick(&names))), Ty::Str),
            (true, false) => {
                let items = (0..length).map(|_| self.str_expr(scope, 1)).collect();
                (Val::Strs(Strs::Lit(items)), Ty::Str)
            }
            (false, true) => (Val::List(List::Var(self.rng.pick(&names))), Ty::Int),
            (false, false) => {
                let items = (0..length)
                    .map(|_| {
                        if self.rng.chance(50) {
                            Expr::Lit(self.literal())
                        } else {
                            self.expr(scope, 2)
                        }
                    })
                    .collect();
                (Val::List(List::Lit(items)), Ty::Int)
            }
        };
        let iterated = match &list {
            Val::List(List::Var(name)) | Val::Strs(Strs::Var(name)) => Some(name.clone()),
            _ => None,
        };
        // Sometimes the loop variable is a local, which may hold an earlier
        // value; the list is never empty, so after the loop it holds the
        // last item.
        let earlier: Vec<String> = scope
            .local_pool
            .iter()
            .filter(|(name, t)| {
                *t == ty
                    && !scope.loop_names.iter().any(|(l, _)| l == name)
                    && !scope.iterated.contains(name)
            })
            .map(|(name, _)| name.clone())
            .collect();
        let local_target = !earlier.is_empty() && self.rng.chance(30);
        let target = if local_target {
            self.rng.pick(&earlier)
        } else {
            target
        };
        if let Some(name) = &iterated {
            scope.iterated.push(name.clone());
        }
        let body = self.loop_body(scope, depth, &target, ty);
        if iterated.is_some() {
            scope.iterated.pop();
        }
        if local_target {
            scope.assigned.insert(target.clone());
        }
        Stmt::ForEach { target, list, body }
    }

    fn function(
        &mut self,
        spec: &ModuleSpec,
        name: String,
        params: Vec<(String, Ty)>,
        callees: Vec<Callee>,
        recursion: Option<&[Callee]>,
    ) -> Function {
        let module_vars = module_vars(spec);
        let mut globals = Vec::new();
        let mut local_pool: Vec<(String, Ty)> = INT_LOCALS
            .iter()
            .map(|s| (s.to_string(), Ty::Int))
            .chain(STR_LOCALS.iter().map(|s| (s.to_string(), Ty::Str)))
            .chain(
                LIST_LOCALS
                    .iter()
                    .map(|(s, len)| (s.to_string(), Ty::List(*len))),
            )
            .chain(
                STRS_LOCALS
                    .iter()
                    .map(|(s, len)| (s.to_string(), Ty::Strs(*len))),
            )
            .collect();
        for (var, ty) in &module_vars {
            if params.iter().any(|(p, _)| p == var) {
                continue;
            }
            let roll = self.rng.below(10);
            match ty {
                Ty::Int if roll <= 3 => globals.push(var.clone()),
                Ty::Str if roll <= 2 => globals.push(var.clone()),
                Ty::List(_) | Ty::Strs(_) if roll <= 1 => globals.push(var.clone()),
                Ty::Rec => {}
                _ if roll == 4 => local_pool.push((var.clone(), *ty)),
                _ => {}
            }
        }
        let mut scope = Scope {
            fixed_params: if recursion.is_some() {
                vec![params[0].0.clone()]
            } else {
                Vec::new()
            },
            params,
            globals: globals.clone(),
            module_vars,
            consts: self.consts.clone(),
            local_pool,
            assigned: BTreeSet::new(),
            loop_names: Vec::new(),
            iterated: Vec::new(),
            callees,
            calls_left: 3,
            loops: 0,
            repeats: name == "main",
        };
        let mut body = Vec::new();
        match recursion {
            None => {
                let length = self.rng.range(2, 6) as usize;
                body = self.block(&mut scope, 2, length);
            }
            Some(group) => {
                let depth_param = scope.params[0].0.clone();
                let length = self.rng.range(0, 2) as usize;
                if length > 0 {
                    body.extend(self.block(&mut scope, 1, length));
                }
                if matches!(body.last(), Some(Stmt::Return(_))) {
                    body.pop();
                }
                let base = vec![Stmt::Return(self.expr(&mut scope, 2))];
                body.push(Stmt::If(
                    vec![(
                        Cond::Cmp("<=", Expr::Var(depth_param.clone()), Expr::Lit(0)),
                        base,
                    )],
                    None,
                ));
                for _ in 0..self.rng.range(1, 2) {
                    let callee = self.rng.pick(group);
                    let mut args = vec![Val::Int(bin(
                        Op::Sub,
                        Expr::Var(depth_param.clone()),
                        Expr::Lit(self.rng.range(1, 2)),
                    ))];
                    for _ in 1..callee.params.len() {
                        args.push(Val::Int(self.expr(&mut scope, 2)));
                    }
                    let targets: Vec<String> = scope
                        .local_pool
                        .iter()
                        .filter(|(_, t)| *t == Ty::Int)
                        .map(|(n, _)| n.clone())
                        .collect();
                    let target = self.rng.pick(&targets);
                    scope.assigned.insert(target.clone());
                    body.push(Stmt::Assign(
                        target,
                        Val::Int(Expr::Call(callee.name, args)),
                    ));
                    let length = self.rng.range(0, 2) as usize;
                    if length > 0 {
                        let more = self.block(&mut scope, 1, length);
                        let returns = matches!(more.last(), Some(Stmt::Return(_)));
                        body.extend(more);
                        if returns {
                            break;
                        }
                    }
                }
            }
        }
        if !matches!(body.last(), Some(Stmt::Return(_))) {
            body.push(Stmt::Return(self.expr(&mut scope, 3)));
        }
        Function {
            name,
            params: scope.params.into_iter().map(|(p, _)| p).collect(),
            globals,
            body,
        }
    }

    fn params(&mut self, spec: &ModuleSpec, count: usize) -> Vec<(String, Ty)> {
        let mut params: Vec<(String, Ty)> = Vec::new();
        for name in ["p", "q"].into_iter().take(count) {
            if self.rng.chance(15) {
                let var = self.rng.pick(spec.ints).to_string();
                if !params.iter().any(|(p, _)| *p == var) {
                    params.push((var, Ty::Int));
                    continue;
                }
            }
            params.push((name.to_string(), Ty::Int));
        }
        if self.rng.chance(20) {
            params.push(("ps".to_string(), Ty::Str));
        }
        if self.rng.chance(20) {
            params.push(("pl".to_string(), Ty::List(PARAM_LIST_LEN)));
        }
        params
    }

    /// One module. `imported` are the functions it can call from the
    /// modules it imports. Returns the module and its own functions, as the
    /// module itself spells them.
    fn module(
        &mut self,
        spec: &ModuleSpec,
        imports: Vec<Import>,
        imported: Vec<Callee>,
    ) -> (Module, Vec<Callee>) {
        let mut vars = Vec::new();
        for (index, var) in spec.ints.iter().enumerate() {
            let value = if index > 0 && self.rng.chance(20) {
                let other = spec.ints[self.rng.below(index)].to_string();
                bin(Op::Add, Expr::Var(other), Expr::Lit(self.literal()))
            } else {
                Expr::Lit(self.literal())
            };
            vars.push((var.to_string(), Val::Int(value)));
        }
        vars.push((
            spec.string.to_string(),
            Val::Str(Str::Lit(self.rng.pick(&STRINGS))),
        ));
        let items = (0..spec.list_len)
            .map(|_| {
                if self.rng.chance(20) {
                    Expr::Var(self.rng.pick(spec.ints).to_string())
                } else {
                    Expr::Lit(self.literal())
                }
            })
            .collect();
        vars.push((spec.list.to_string(), Val::List(List::Lit(items))));
        let strings = (0..spec.strs_len)
            .map(|_| Str::Lit(self.rng.pick(&STRINGS)))
            .collect();
        vars.push((spec.strs.to_string(), Val::Strs(Strs::Lit(strings))));
        vars.push((
            spec.rec.to_string(),
            Val::Rec(
                Str::Lit(self.rng.pick(&STRINGS)),
                Str::Lit(self.rng.pick(&STRINGS)),
            ),
        ));

        // Module-level code that runs at load: a loop over the module's
        // list, or a `while`, changing a number of the module.
        let mut code = Vec::new();
        if self.rng.chance(25) {
            let target = self.rng.pick(spec.ints).to_string();
            let op = self.rng.pick(&[Op::Add, Op::Sub, Op::Mul]);
            if self.rng.chance(60) {
                let item = self.fresh("v");
                let value = if self.rng.chance(70) {
                    Expr::Var(item.clone())
                } else {
                    bin(Op::Add, Expr::Var(item.clone()), Expr::Lit(self.literal()))
                };
                code.push(Stmt::ForEach {
                    target: item,
                    list: Val::List(List::Var(spec.list.to_string())),
                    body: vec![Stmt::Compound(target, op, value)],
                });
            } else {
                code.push(Stmt::While {
                    counter: self.fresh("w"),
                    limit: Expr::Lit(self.rng.range(0, 3)),
                    extra: None,
                    body: vec![Stmt::Compound(target, op, Expr::Lit(self.literal()))],
                });
            }
        }
        // Constants declared below the functions that read them.
        self.consts.clear();
        let mut consts = Vec::new();
        if self.rng.chance(35) {
            self.counter += 1;
            let name = format!("K{}", self.counter);
            consts.push((name.clone(), self.literal()));
            self.consts.push(name);
        }

        let mut functions = Vec::new();
        let mut own: Vec<Callee> = Vec::new();
        let entry = spec.name == MAIN.name;
        let plain = if entry {
            self.rng.range(1, 3)
        } else {
            self.rng.range(1, 2)
        };
        let callees = |own: &[Callee]| {
            let mut callees = imported.clone();
            callees.extend(own.iter().cloned());
            callees
        };
        for _ in 0..plain {
            self.functions += 1;
            let name = format!("f{}", self.functions);
            let count = self.rng.below(3);
            let params = self.params(spec, count);
            let types = params.iter().map(|(_, t)| *t).collect();
            functions.push(self.function(spec, name.clone(), params, callees(&own), None));
            own.push(Callee {
                name,
                params: types,
                recursive: false,
            });
        }
        if self.rng.chance(if entry { 70 } else { 40 }) {
            let mutual = self.rng.chance(40);
            let group: Vec<Callee> = (0..if mutual { 2 } else { 1 })
                .map(|_| {
                    self.functions += 1;
                    let arity = self.rng.range(1, 2) as usize;
                    Callee {
                        name: format!("r{}", self.functions),
                        params: vec![Ty::Int; arity],
                        recursive: true,
                    }
                })
                .collect();
            for callee in &group {
                let mut params = vec![("n".to_string(), Ty::Int)];
                if callee.params.len() > 1 {
                    params.push(("x".to_string(), Ty::Int));
                }
                functions.push(self.function(
                    spec,
                    callee.name.clone(),
                    params,
                    callees(&own),
                    Some(&group),
                ));
            }
            own.extend(group);
        }
        if entry {
            functions.push(self.function(
                spec,
                "main".to_string(),
                Vec::new(),
                callees(&own),
                None,
            ));
        }
        (
            Module {
                name: spec.name,
                imports,
                vars,
                code,
                functions,
                consts,
            },
            own,
        )
    }

    /// An import of `module` and the functions it makes callable, as the
    /// importer spells them.
    fn import(&mut self, module: &'static str, exported: &[Callee]) -> (Import, Vec<Callee>) {
        if self.rng.chance(50) {
            let callees = exported
                .iter()
                .map(|callee| Callee {
                    name: format!("{module}.{}", callee.name),
                    ..callee.clone()
                })
                .collect();
            return (Import::Module(module), callees);
        }
        let mut chosen: Vec<Callee> = exported
            .iter()
            .filter(|_| self.rng.chance(70))
            .cloned()
            .collect();
        if chosen.is_empty() {
            chosen.push(self.rng.pick(exported));
        }
        let names = chosen.iter().map(|callee| callee.name.clone()).collect();
        (Import::Names(module, names), chosen)
    }

    fn program(&mut self) -> Program {
        let files = match self.rng.below(10) {
            0..=5 => 1,
            6..=8 => 2,
            _ => 3,
        };
        let mut modules = Vec::new();
        let mut extra_functions = None;
        if files == 3 {
            let (module, exported) = self.module(&EXTRA, Vec::new(), Vec::new());
            modules.push(module);
            extra_functions = Some(exported);
        }
        let mut main_imports = Vec::new();
        let mut main_callees = Vec::new();
        if files >= 2 {
            let mut imports = Vec::new();
            let mut callees = Vec::new();
            if let Some(exported) = &extra_functions {
                if self.rng.chance(50) {
                    let (import, imported) = self.import(EXTRA.name, exported);
                    imports.push(import);
                    callees.extend(imported);
                }
            }
            let (module, exported) = self.module(&UTIL, imports, callees);
            modules.push(module);
            let (import, imported) = self.import(UTIL.name, &exported);
            main_imports.push(import);
            main_callees.extend(imported);
        }
        if let Some(exported) = &extra_functions {
            if self.rng.chance(50) {
                let (import, imported) = self.import(EXTRA.name, exported);
                main_imports.push(import);
                main_callees.extend(imported);
            }
        }
        let (module, _) = self.module(&MAIN, main_imports, main_callees);
        modules.push(module);
        Program { modules }
    }
}

fn module_vars(spec: &ModuleSpec) -> Vec<(String, Ty)> {
    let mut vars: Vec<(String, Ty)> = spec.ints.iter().map(|v| (v.to_string(), Ty::Int)).collect();
    vars.push((spec.string.to_string(), Ty::Str));
    vars.push((spec.list.to_string(), Ty::List(spec.list_len)));
    vars.push((spec.strs.to_string(), Ty::Strs(spec.strs_len)));
    vars.push((spec.rec.to_string(), Ty::Rec));
    vars
}

// ---------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Language {
    Cobble,
    Python,
}

/// Cobble text with only the parentheses that precedence needs, so the
/// parser's precedence and associativity are part of the test.
fn cobble_expr(expr: &Expr) -> String {
    fn operand(expr: &Expr, parent: Op, right: bool) -> String {
        let text = cobble_expr(expr);
        let Expr::Bin(op, ..) = expr else {
            return text;
        };
        let (child, outer) = (op.precedence(), parent.precedence());
        // `^` groups to the right, the other operators to the left.
        let needs =
            child < outer || (child == outer && (if parent == Op::Pow { !right } else { right }));
        if needs {
            format!("({text})")
        } else {
            text
        }
    }
    match expr {
        Expr::Lit(value) if *value < 0 => format!("({value})"),
        Expr::Lit(value) => value.to_string(),
        Expr::Var(name) => name.clone(),
        Expr::Neg(inner) => match inner.as_ref() {
            Expr::Lit(_) | Expr::Var(_) | Expr::Call(..) | Expr::Item(..) | Expr::Len(_) => {
                format!("(-{})", cobble_expr(inner))
            }
            _ => format!("(-({}))", cobble_expr(inner)),
        },
        Expr::Bin(op, left, right) => format!(
            "{} {} {}",
            operand(left, *op, false),
            op.symbol(),
            operand(right, *op, true)
        ),
        Expr::Call(name, args) => format!("{name}({})", vals(args, Language::Cobble)),
        Expr::Item(name, index) => format!("{name}[{}]", cobble_expr(index)),
        Expr::Len(name) => format!("len({name})"),
        Expr::Pick(condition, value, otherwise) => format!(
            "({} if {} else {})",
            cobble_expr(value),
            cobble_cond(condition, 0),
            cobble_expr(otherwise)
        ),
        Expr::Abs(value) => format!("abs({})", cobble_expr(value)),
        Expr::Extreme(max, left, right) => format!(
            "{}({}, {})",
            if *max { "max" } else { "min" },
            cobble_expr(left),
            cobble_expr(right)
        ),
        Expr::Either(or, left, right) => format!(
            "({} {} {})",
            cobble_expr(left),
            if *or { "or" } else { "and" },
            cobble_expr(right)
        ),
        Expr::Sum(name) => format!("sum({name})"),
        Expr::ListExtreme(max, name) => {
            format!("{}({name})", if *max { "max" } else { "min" })
        }
        Expr::Count(name, value) => format!("{name}.count({})", cobble_expr(value)),
        Expr::StrLen(text) => format!("len({})", str_text(text, Language::Cobble)),
    }
}

fn python_expr(expr: &Expr) -> String {
    match expr {
        Expr::Lit(value) if *value < 0 => format!("({value})"),
        Expr::Lit(value) => value.to_string(),
        Expr::Var(name) => name.clone(),
        Expr::Neg(inner) => format!("neg({})", python_expr(inner)),
        Expr::Bin(op, left, right) => format!(
            "{}({}, {})",
            op.helper(),
            python_expr(left),
            python_expr(right)
        ),
        Expr::Call(name, args) => format!("{name}({})", vals(args, Language::Python)),
        Expr::Item(name, index) => format!("{name}[{}]", python_expr(index)),
        Expr::Len(name) => format!("len({name})"),
        Expr::Pick(condition, value, otherwise) => format!(
            "({} if {} else {})",
            python_expr(value),
            python_cond(condition),
            python_expr(otherwise)
        ),
        Expr::Abs(value) => format!("W(abs({}))", python_expr(value)),
        Expr::Extreme(max, left, right) => format!(
            "{}({}, {})",
            if *max { "max" } else { "min" },
            python_expr(left),
            python_expr(right)
        ),
        Expr::Either(or, left, right) => format!(
            "({} {} {})",
            python_expr(left),
            if *or { "or" } else { "and" },
            python_expr(right)
        ),
        Expr::Sum(name) => format!("W(sum({name}))"),
        Expr::ListExtreme(max, name) => {
            format!("{}({name})", if *max { "max" } else { "min" })
        }
        Expr::Count(name, value) => format!("{name}.count({})", python_expr(value)),
        Expr::StrLen(text) => format!("len({})", str_text(text, Language::Python)),
    }
}

fn expr_text(expr: &Expr, language: Language) -> String {
    match language {
        Language::Cobble => cobble_expr(expr),
        Language::Python => python_expr(expr),
    }
}

/// `name.field`, which Python spells `name["field"]`.
fn field_text(name: &str, field: &str, key: bool, language: Language) -> String {
    if key || language == Language::Python {
        format!("{name}[\"{field}\"]")
    } else {
        format!("{name}.{field}")
    }
}

fn str_text(text: &Str, language: Language) -> String {
    match text {
        Str::Lit(text) => format!("\"{text}\""),
        Str::Var(name) => name.clone(),
        Str::Char(name, index) | Str::Item(name, index) => {
            format!("{name}[{}]", expr_text(index, language))
        }
        Str::ItemChar(name, index, at) => format!(
            "{name}[{}][{}]",
            expr_text(index, language),
            expr_text(at, language)
        ),
        Str::Field(name, field, key) => field_text(name, field, *key, language),
        Str::FieldChar(name, field, key, at) => format!(
            "{}[{}]",
            field_text(name, field, *key, language),
            expr_text(at, language)
        ),
        Str::Join(left, right) => format!(
            "({} + {})",
            str_text(left, language),
            str_text(right, language)
        ),
        Str::Repeat(text, count) => format!(
            "({} * ({}))",
            str_text(text, language),
            expr_text(count, language)
        ),
    }
}

fn place_text(place: &Place, language: Language) -> String {
    match place {
        Place::Var(name) => name.clone(),
        Place::Item(name, index) => format!("{name}[{}]", expr_text(index, language)),
        Place::Field(name, field, key) => field_text(name, field, *key, language),
    }
}

fn val_text(val: &Val, language: Language) -> String {
    match val {
        Val::Int(expr) => expr_text(expr, language),
        Val::Str(text) => str_text(text, language),
        Val::Rec(first, second) => format!(
            "{{\"s\": {}, \"t\": {}}}",
            str_text(first, language),
            str_text(second, language)
        ),
        Val::List(list) => list_text(list, language),
        Val::Strs(Strs::Lit(items)) => format!(
            "[{}]",
            items
                .iter()
                .map(|item| val_text(&Val::Str(item.clone()), language))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Val::Strs(Strs::Var(name)) => name.clone(),
    }
}

fn list_text(list: &List, language: Language) -> String {
    match list {
        List::Lit(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|item| expr_text(item, language))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        List::Var(name) => name.clone(),
    }
}

fn vals(args: &[Val], language: Language) -> String {
    args.iter()
        .map(|arg| val_text(arg, language))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `level`: 0 for `or`, 1 for `and`, 2 for `not`, 3 for a comparison.
fn cobble_cond(cond: &Cond, level: u8) -> String {
    let (own, text) = match cond {
        Cond::Cmp(op, left, right) => (
            3,
            format!("{} {op} {}", cobble_expr(left), cobble_expr(right)),
        ),
        Cond::Truthy(expr) => (3, cobble_expr(expr)),
        Cond::Not(inner, parens) => {
            let operand = cobble_cond(inner, 2);
            let operand = if *parens && !operand.starts_with('(') {
                format!("({operand})")
            } else {
                operand
            };
            (2, format!("not {operand}"))
        }
        Cond::And(left, right) => (
            1,
            format!("{} and {}", cobble_cond(left, 1), cobble_cond(right, 2)),
        ),
        Cond::Or(left, right) => (
            0,
            format!("{} or {}", cobble_cond(left, 0), cobble_cond(right, 1)),
        ),
        Cond::In(value, name, negated) => (
            3,
            format!(
                "{} {} {name}",
                cobble_expr(value),
                if *negated { "not in" } else { "in" }
            ),
        ),
        Cond::Text(op, left, right) => (
            3,
            format!(
                "{} {op} {}",
                val_text(&Val::Str(left.clone()), Language::Cobble),
                val_text(&Val::Str(right.clone()), Language::Cobble)
            ),
        ),
        Cond::Chain(left, first, middle, second, right) => (
            3,
            format!(
                "{} {first} {} {second} {}",
                cobble_expr(left),
                cobble_expr(middle),
                cobble_expr(right)
            ),
        ),
        Cond::TextTruth(text, false) => (3, str_text(text, Language::Cobble)),
        Cond::TextTruth(text, true) => (2, format!("not {}", str_text(text, Language::Cobble))),
    };
    if own < level {
        format!("({text})")
    } else {
        text
    }
}

fn python_cond(cond: &Cond) -> String {
    match cond {
        Cond::Cmp(op, left, right) => {
            format!("({} {op} {})", python_expr(left), python_expr(right))
        }
        Cond::Truthy(expr) => format!("({} != 0)", python_expr(expr)),
        Cond::Not(inner, _) => format!("(not {})", python_cond(inner)),
        Cond::And(left, right) => format!("({} and {})", python_cond(left), python_cond(right)),
        Cond::Or(left, right) => format!("({} or {})", python_cond(left), python_cond(right)),
        Cond::In(value, name, negated) => format!(
            "({} {} {name})",
            python_expr(value),
            if *negated { "not in" } else { "in" }
        ),
        Cond::Text(op, left, right) => format!(
            "({} {op} {})",
            val_text(&Val::Str(left.clone()), Language::Python),
            val_text(&Val::Str(right.clone()), Language::Python)
        ),
        Cond::Chain(left, first, middle, second, right) => format!(
            "({} {first} {} {second} {})",
            python_expr(left),
            python_expr(middle),
            python_expr(right)
        ),
        Cond::TextTruth(text, negated) => format!(
            "({}bool({}))",
            if *negated { "not " } else { "" },
            str_text(text, Language::Python)
        ),
    }
}

fn print_block(out: &mut String, body: &[Stmt], indent: usize, language: Language) {
    let pad = "    ".repeat(indent);
    let expr = |expr: &Expr| expr_text(expr, language);
    let cond = |cond: &Cond| match language {
        Language::Cobble => cobble_cond(cond, 0),
        Language::Python => python_cond(cond),
    };
    for statement in body {
        match statement {
            Stmt::Assign(target, value) => {
                writeln!(out, "{pad}{target} = {}", val_text(value, language)).unwrap();
            }
            Stmt::Compound(target, op, value) => match language {
                Language::Cobble => {
                    writeln!(out, "{pad}{target} {}= {}", op.symbol(), expr(value)).unwrap()
                }
                Language::Python => writeln!(
                    out,
                    "{pad}{target} = {}({target}, {})",
                    op.helper(),
                    expr(value)
                )
                .unwrap(),
            },
            Stmt::Swap(first, second) => {
                writeln!(out, "{pad}{first}, {second} = {second}, {first}").unwrap();
            }
            Stmt::Append(target, value) => {
                writeln!(out, "{pad}{target}.append({})", expr(value)).unwrap();
            }
            Stmt::SetItem(target, index, value) => {
                writeln!(out, "{pad}{target}[{}] = {}", expr(index), expr(value)).unwrap();
            }
            Stmt::SetStr(target, index, value) => {
                let value = val_text(&Val::Str(value.clone()), language);
                writeln!(out, "{pad}{target}[{}] = {value}", expr(index)).unwrap();
            }
            Stmt::Call(name, args) => {
                writeln!(
                    out,
                    "{pad}{}",
                    expr(&Expr::Call(name.clone(), args.clone()))
                )
                .unwrap();
            }
            Stmt::Raw(action, target, value) => match language {
                Language::Cobble => writeln!(
                    out,
                    "{pad}/scoreboard players {action} {target} temp {value}"
                )
                .unwrap(),
                Language::Python => {
                    let value = match *action {
                        "add" => format!("add({target}, {value})"),
                        "remove" => format!("sub({target}, {value})"),
                        _ => value.to_string(),
                    };
                    writeln!(out, "{pad}{target} = {value}").unwrap();
                }
            },
            Stmt::Say(names) => {
                let text: Vec<String> = names.iter().map(|n| format!("{{{n}}}")).collect();
                match language {
                    Language::Cobble => writeln!(out, "{pad}/say v {}", text.join(" ")).unwrap(),
                    Language::Python => {
                        writeln!(out, "{pad}LOG.append(f\"v {}\")", text.join(" ")).unwrap()
                    }
                }
            }
            Stmt::If(branches, else_branch) => {
                for (index, (condition, body)) in branches.iter().enumerate() {
                    let keyword = if index == 0 { "if" } else { "elif" };
                    writeln!(out, "{pad}{keyword} {}:", cond(condition)).unwrap();
                    print_block(out, body, indent + 1, language);
                }
                if let Some(body) = else_branch {
                    writeln!(out, "{pad}else:").unwrap();
                    print_block(out, body, indent + 1, language);
                }
            }
            Stmt::While {
                counter,
                limit,
                extra,
                body,
            } => {
                writeln!(out, "{pad}{counter} = 0").unwrap();
                let mut header = match language {
                    Language::Cobble => format!("{counter} < {}", expr(limit)),
                    Language::Python => format!("({counter} < {})", expr(limit)),
                };
                if let Some(extra) = extra {
                    let extra = match language {
                        Language::Cobble => cobble_cond(extra, 2),
                        Language::Python => python_cond(extra),
                    };
                    header = format!("{header} and {extra}");
                }
                writeln!(out, "{pad}while {header}:").unwrap();
                print_block(out, body, indent + 1, language);
                if !matches!(body.last(), Some(Stmt::Return(_))) {
                    match language {
                        Language::Cobble => writeln!(out, "{pad}    {counter} += 1").unwrap(),
                        Language::Python => {
                            writeln!(out, "{pad}    {counter} = add({counter}, 1)").unwrap()
                        }
                    }
                }
            }
            Stmt::For { target, args, body } => {
                let args: Vec<String> = args.iter().map(expr).collect();
                writeln!(out, "{pad}for {target} in range({}):", args.join(", ")).unwrap();
                print_block(out, body, indent + 1, language);
            }
            Stmt::ForEach { target, list, body } => {
                writeln!(out, "{pad}for {target} in {}:", val_text(list, language)).unwrap();
                print_block(out, body, indent + 1, language);
            }
            Stmt::Return(value) => writeln!(out, "{pad}return {}", expr(value)).unwrap(),
            Stmt::SetField(name, field, key, value) => writeln!(
                out,
                "{pad}{} = {}",
                field_text(name, field, *key, language),
                str_text(value, language)
            )
            .unwrap(),
            Stmt::JoinTo(place, value) => writeln!(
                out,
                "{pad}{} += {}",
                place_text(place, language),
                str_text(value, language)
            )
            .unwrap(),
            Stmt::RepeatAt(place, count) => writeln!(
                out,
                "{pad}{} *= {}",
                place_text(place, language),
                expr(count)
            )
            .unwrap(),
        }
    }
}

fn print_module(module: &Module, language: Language) -> String {
    let mut out = String::new();
    for import in &module.imports {
        match import {
            Import::Module(name) => writeln!(out, "import {name}").unwrap(),
            Import::Names(name, names) => {
                writeln!(out, "from {name} import {}", names.join(", ")).unwrap()
            }
        }
    }
    for (name, value) in &module.vars {
        writeln!(out, "{name} = {}", val_text(value, language)).unwrap();
    }
    print_block(&mut out, &module.code, 0, language);
    for function in &module.functions {
        writeln!(
            out,
            "\ndef {}({}):",
            function.name,
            function.params.join(", ")
        )
        .unwrap();
        if !function.globals.is_empty() {
            writeln!(out, "    global {}", function.globals.join(", ")).unwrap();
        }
        print_block(&mut out, &function.body, 1, language);
    }
    for (name, value) in &module.consts {
        match language {
            Language::Cobble => writeln!(out, "\nconst {name} = {value}").unwrap(),
            Language::Python => writeln!(out, "\n{name} = {value}").unwrap(),
        }
    }
    out
}

/// Every module of `program`, each after a line naming its file.
fn print_program(program: &Program, language: Language) -> String {
    if let [module] = program.modules.as_slice() {
        return print_module(module, language);
    }
    let extension = match language {
        Language::Cobble => "cbl",
        Language::Python => "py",
    };
    program
        .modules
        .iter()
        .map(|module| {
            format!(
                "# {}.{extension}\n{}",
                module.name,
                print_module(module, language)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Shrinking
// ---------------------------------------------------------------------------

fn functions_mut(program: &mut Program) -> impl Iterator<Item = &mut Function> {
    program
        .modules
        .iter_mut()
        .flat_map(|module| module.functions.iter_mut())
}

/// Calls `visit` on every block, each block before the blocks inside it.
fn for_each_block(program: &mut Program, visit: &mut dyn FnMut(&mut Vec<Stmt>)) {
    fn walk(body: &mut Vec<Stmt>, visit: &mut dyn FnMut(&mut Vec<Stmt>)) {
        visit(body);
        for statement in body.iter_mut() {
            match statement {
                Stmt::If(branches, else_branch) => {
                    for (_, body) in branches.iter_mut() {
                        walk(body, visit);
                    }
                    if let Some(body) = else_branch {
                        walk(body, visit);
                    }
                }
                Stmt::While { body, .. } | Stmt::For { body, .. } | Stmt::ForEach { body, .. } => {
                    walk(body, visit)
                }
                _ => {}
            }
        }
    }
    for function in functions_mut(program) {
        walk(&mut function.body, visit);
    }
}

/// Calls `visit` on every number expression, parents before children. The
/// flag marks loop bounds and exponents, which only shrink to small
/// literals so that no candidate loops for long.
fn for_each_expr(program: &mut Program, visit: &mut dyn FnMut(&mut Expr, bool)) {
    fn expr(e: &mut Expr, bound: bool, visit: &mut dyn FnMut(&mut Expr, bool)) {
        visit(e, bound);
        match e {
            Expr::Neg(inner) => expr(inner, bound, visit),
            // An exponent counts as a bound too: a large one makes the
            // runtime power loop run for long.
            Expr::Bin(op, left, right) => {
                let exponent = *op == Op::Pow;
                expr(left, bound, visit);
                expr(right, bound || exponent, visit);
            }
            Expr::Call(_, args) => args.iter_mut().for_each(|arg| val(arg, visit)),
            Expr::Item(_, index) | Expr::Count(_, index) => expr(index, bound, visit),
            Expr::Pick(condition, value, otherwise) => {
                cond(condition, visit);
                expr(value, bound, visit);
                expr(otherwise, bound, visit);
            }
            Expr::Abs(inner) => expr(inner, bound, visit),
            Expr::Extreme(_, left, right) | Expr::Either(_, left, right) => {
                expr(left, bound, visit);
                expr(right, bound, visit);
            }
            Expr::Lit(_) | Expr::Var(_) | Expr::Len(_) | Expr::Sum(_) | Expr::ListExtreme(..) => {}
            Expr::StrLen(text) => string(text, visit),
        }
    }
    fn val(v: &mut Val, visit: &mut dyn FnMut(&mut Expr, bool)) {
        match v {
            Val::Int(e) => expr(e, false, visit),
            Val::Str(s) => string(s, visit),
            Val::Rec(first, second) => {
                string(first, visit);
                string(second, visit);
            }
            Val::List(l) => list_items(l, visit),
            Val::Strs(Strs::Lit(items)) => items.iter_mut().for_each(|item| string(item, visit)),
            Val::Strs(Strs::Var(_)) => {}
        }
    }
    fn string(s: &mut Str, visit: &mut dyn FnMut(&mut Expr, bool)) {
        match s {
            Str::Char(_, index) | Str::Item(_, index) | Str::FieldChar(_, _, _, index) => {
                expr(index, false, visit)
            }
            Str::ItemChar(_, index, at) => {
                expr(index, false, visit);
                expr(at, false, visit);
            }
            Str::Join(left, right) => {
                string(left, visit);
                string(right, visit);
            }
            // A large count makes long text.
            Str::Repeat(text, count) => {
                string(text, visit);
                expr(count, true, visit);
            }
            Str::Lit(_) | Str::Var(_) | Str::Field(..) => {}
        }
    }
    fn place(p: &mut Place, visit: &mut dyn FnMut(&mut Expr, bool)) {
        if let Place::Item(_, index) = p {
            expr(index, false, visit);
        }
    }
    fn list_items(l: &mut List, visit: &mut dyn FnMut(&mut Expr, bool)) {
        if let List::Lit(items) = l {
            items.iter_mut().for_each(|item| expr(item, false, visit));
        }
    }
    fn cond(c: &mut Cond, visit: &mut dyn FnMut(&mut Expr, bool)) {
        match c {
            Cond::Cmp(_, left, right) => {
                expr(left, false, visit);
                expr(right, false, visit);
            }
            Cond::Truthy(e) => expr(e, false, visit),
            Cond::Not(inner, _) => cond(inner, visit),
            Cond::And(left, right) | Cond::Or(left, right) => {
                cond(left, visit);
                cond(right, visit);
            }
            Cond::In(value, ..) => expr(value, false, visit),
            Cond::Text(_, left, right) => {
                string(left, visit);
                string(right, visit);
            }
            Cond::Chain(left, _, _, _, right) => {
                expr(left, false, visit);
                expr(right, false, visit);
            }
            Cond::TextTruth(text, _) => string(text, visit),
        }
    }
    fn block(body: &mut [Stmt], visit: &mut dyn FnMut(&mut Expr, bool)) {
        for statement in body {
            match statement {
                Stmt::Assign(_, v) => val(v, visit),
                Stmt::Compound(_, _, e) | Stmt::Return(e) | Stmt::Append(_, e) => {
                    expr(e, false, visit)
                }
                Stmt::SetItem(_, index, value) => {
                    expr(index, false, visit);
                    expr(value, false, visit);
                }
                Stmt::SetStr(_, index, value) => {
                    expr(index, false, visit);
                    string(value, visit);
                }
                Stmt::Call(_, args) => args.iter_mut().for_each(|arg| val(arg, visit)),
                Stmt::If(branches, else_branch) => {
                    for (condition, body) in branches.iter_mut() {
                        cond(condition, visit);
                        block(body, visit);
                    }
                    if let Some(body) = else_branch {
                        block(body, visit);
                    }
                }
                Stmt::While {
                    limit, extra, body, ..
                } => {
                    expr(limit, true, visit);
                    if let Some(extra) = extra {
                        cond(extra, visit);
                    }
                    block(body, visit);
                }
                Stmt::For { args, body, .. } => {
                    args.iter_mut().for_each(|arg| expr(arg, true, visit));
                    block(body, visit);
                }
                Stmt::ForEach { list, body, .. } => {
                    val(list, visit);
                    block(body, visit);
                }
                Stmt::Raw(..) | Stmt::Say(_) | Stmt::Swap(..) => {}
                Stmt::SetField(_, _, _, value) => string(value, visit),
                Stmt::JoinTo(target, value) => {
                    place(target, visit);
                    string(value, visit);
                }
                Stmt::RepeatAt(target, count) => {
                    place(target, visit);
                    expr(count, true, visit);
                }
            }
        }
    }
    for module in &mut program.modules {
        for (_, value) in &mut module.vars {
            val(value, visit);
        }
        for function in &mut module.functions {
            block(&mut function.body, visit);
        }
    }
}

/// Calls `visit` on every condition of an `if` or `elif`, parents first.
fn for_each_cond(program: &mut Program, visit: &mut dyn FnMut(&mut Cond)) {
    fn cond(c: &mut Cond, visit: &mut dyn FnMut(&mut Cond)) {
        visit(c);
        match c {
            Cond::Not(inner, _) => cond(inner, visit),
            Cond::And(left, right) | Cond::Or(left, right) => {
                cond(left, visit);
                cond(right, visit);
            }
            Cond::Cmp(..)
            | Cond::Truthy(_)
            | Cond::In(..)
            | Cond::Text(..)
            | Cond::Chain(..)
            | Cond::TextTruth(..) => {}
        }
    }
    for_each_block(program, &mut |body| {
        for statement in body.iter_mut() {
            if let Stmt::If(branches, _) = statement {
                for (condition, _) in branches.iter_mut() {
                    cond(condition, visit);
                }
            }
        }
    });
}

/// Programs one step smaller than `program`. Some are not valid Python
/// (a removed assignment leaves a read unbound); the caller drops those.
fn shrink_candidates(program: &Program) -> Vec<Program> {
    let mut candidates = smaller_programs(program);
    // Generated functions end with `return`. Without it Python returns
    // `None` where Cobble returns 0, which is not the difference to find.
    for candidate in &mut candidates {
        for function in functions_mut(candidate) {
            if !matches!(function.body.last(), Some(Stmt::Return(_))) {
                function.body.push(Stmt::Return(Expr::Lit(0)));
            }
        }
    }
    // Removing a final `return 0` gives the same program back.
    let original = print_program(program, Language::Cobble);
    candidates.retain(|candidate| print_program(candidate, Language::Cobble) != original);
    // Python only looks a name up when the code runs, but Cobble rejects a
    // call to a function, or a read of a variable, that doesn't exist.
    candidates.retain(|candidate| calls_resolve(candidate) && names_resolve(candidate));
    candidates
}

/// Whether every variable a function reads is one of its parameters, a name
/// it assigns or loops over somewhere, or a variable of its module.
fn names_resolve(program: &Program) -> bool {
    fn expr_reads(expr: &Expr, reads: &mut Vec<String>) {
        match expr {
            Expr::Var(name) | Expr::Len(name) => reads.push(name.clone()),
            Expr::Item(name, index) => {
                reads.push(name.clone());
                expr_reads(index, reads);
            }
            Expr::Neg(inner) => expr_reads(inner, reads),
            Expr::Bin(_, left, right) => {
                expr_reads(left, reads);
                expr_reads(right, reads);
            }
            Expr::Call(_, args) => args.iter().for_each(|arg| val_reads(arg, reads)),
            Expr::Lit(_) => {}
            Expr::Pick(condition, value, otherwise) => {
                cond_reads(condition, reads);
                expr_reads(value, reads);
                expr_reads(otherwise, reads);
            }
            Expr::Abs(inner) => expr_reads(inner, reads),
            Expr::Extreme(_, left, right) | Expr::Either(_, left, right) => {
                expr_reads(left, reads);
                expr_reads(right, reads);
            }
            Expr::Sum(name) | Expr::ListExtreme(_, name) => reads.push(name.clone()),
            Expr::Count(name, value) => {
                reads.push(name.clone());
                expr_reads(value, reads);
            }
            Expr::StrLen(text) => str_reads(text, reads),
        }
    }
    fn str_reads(value: &Str, reads: &mut Vec<String>) {
        match value {
            Str::Var(name) | Str::Field(name, ..) => reads.push(name.clone()),
            Str::Char(name, index) | Str::Item(name, index) | Str::FieldChar(name, _, _, index) => {
                reads.push(name.clone());
                expr_reads(index, reads);
            }
            Str::ItemChar(name, index, at) => {
                reads.push(name.clone());
                expr_reads(index, reads);
                expr_reads(at, reads);
            }
            Str::Join(left, right) => {
                str_reads(left, reads);
                str_reads(right, reads);
            }
            Str::Repeat(text, count) => {
                str_reads(text, reads);
                expr_reads(count, reads);
            }
            Str::Lit(_) => {}
        }
    }
    fn place_reads(place: &Place, reads: &mut Vec<String>) {
        match place {
            Place::Var(name) | Place::Field(name, ..) => reads.push(name.clone()),
            Place::Item(name, index) => {
                reads.push(name.clone());
                expr_reads(index, reads);
            }
        }
    }
    fn val_reads(value: &Val, reads: &mut Vec<String>) {
        match value {
            Val::Int(expr) => expr_reads(expr, reads),
            Val::Str(value) => str_reads(value, reads),
            Val::List(List::Lit(items)) => items.iter().for_each(|item| expr_reads(item, reads)),
            Val::Strs(Strs::Lit(items)) => items.iter().for_each(|item| str_reads(item, reads)),
            Val::List(List::Var(name)) | Val::Strs(Strs::Var(name)) => reads.push(name.clone()),
            Val::Rec(first, second) => {
                str_reads(first, reads);
                str_reads(second, reads);
            }
        }
    }
    fn cond_reads(cond: &Cond, reads: &mut Vec<String>) {
        match cond {
            Cond::Cmp(_, left, right) => {
                expr_reads(left, reads);
                expr_reads(right, reads);
            }
            Cond::Truthy(expr) => expr_reads(expr, reads),
            Cond::Not(inner, _) => cond_reads(inner, reads),
            Cond::And(left, right) | Cond::Or(left, right) => {
                cond_reads(left, reads);
                cond_reads(right, reads);
            }
            Cond::In(value, name, _) => {
                expr_reads(value, reads);
                reads.push(name.clone());
            }
            Cond::Text(_, left, right) => {
                str_reads(left, reads);
                str_reads(right, reads);
            }
            Cond::Chain(left, _, middle, _, right) => {
                expr_reads(left, reads);
                expr_reads(middle, reads);
                expr_reads(right, reads);
            }
            Cond::TextTruth(text, _) => str_reads(text, reads),
        }
    }
    fn walk(body: &[Stmt], reads: &mut Vec<String>, defined: &mut Vec<String>) {
        for statement in body {
            match statement {
                Stmt::Assign(target, value) => {
                    defined.push(target.clone());
                    val_reads(value, reads);
                }
                Stmt::Compound(target, _, value) => {
                    reads.push(target.clone());
                    expr_reads(value, reads);
                }
                Stmt::SetItem(target, index, value) => {
                    reads.push(target.clone());
                    expr_reads(index, reads);
                    expr_reads(value, reads);
                }
                Stmt::SetStr(target, index, value) => {
                    reads.push(target.clone());
                    expr_reads(index, reads);
                    str_reads(value, reads);
                }
                Stmt::Call(_, args) => args.iter().for_each(|arg| val_reads(arg, reads)),
                Stmt::SetField(target, _, _, value) => {
                    reads.push(target.clone());
                    str_reads(value, reads);
                }
                Stmt::JoinTo(target, value) => {
                    place_reads(target, reads);
                    str_reads(value, reads);
                }
                Stmt::RepeatAt(target, count) => {
                    place_reads(target, reads);
                    expr_reads(count, reads);
                }
                Stmt::Raw(_, target, _) => reads.push(target.clone()),
                Stmt::Say(names) => reads.extend(names.iter().cloned()),
                Stmt::If(branches, else_branch) => {
                    for (condition, body) in branches {
                        cond_reads(condition, reads);
                        walk(body, reads, defined);
                    }
                    if let Some(body) = else_branch {
                        walk(body, reads, defined);
                    }
                }
                Stmt::While {
                    counter,
                    limit,
                    extra,
                    body,
                } => {
                    defined.push(counter.clone());
                    expr_reads(limit, reads);
                    if let Some(extra) = extra {
                        cond_reads(extra, reads);
                    }
                    walk(body, reads, defined);
                }
                Stmt::For { target, args, body } => {
                    defined.push(target.clone());
                    args.iter().for_each(|arg| expr_reads(arg, reads));
                    walk(body, reads, defined);
                }
                Stmt::ForEach { target, list, body } => {
                    defined.push(target.clone());
                    val_reads(list, reads);
                    walk(body, reads, defined);
                }
                Stmt::Return(value) => expr_reads(value, reads),
                Stmt::Swap(first, second) => {
                    reads.push(first.clone());
                    reads.push(second.clone());
                }
                Stmt::Append(target, value) => {
                    reads.push(target.clone());
                    expr_reads(value, reads);
                }
            }
        }
    }
    program.modules.iter().all(|module| {
        let module_vars: Vec<&String> = module
            .vars
            .iter()
            .map(|(name, _)| name)
            .chain(module.consts.iter().map(|(name, _)| name))
            .collect();
        module.functions.iter().all(|function| {
            let mut reads = Vec::new();
            let mut defined = function.params.clone();
            walk(&function.body, &mut reads, &mut defined);
            reads
                .iter()
                .all(|name| defined.contains(name) || module_vars.contains(&name))
        })
    })
}

/// Whether every call names a function that its module defines or imports.
fn calls_resolve(program: &Program) -> bool {
    let defined = |module: &str, function: &str| {
        program
            .modules
            .iter()
            .any(|m| m.name == module && m.functions.iter().any(|f| f.name == function))
    };
    program.modules.iter().all(|module| {
        let mut copy = Program {
            modules: vec![module.clone()],
        };
        let mut names = Vec::new();
        for_each_expr(&mut copy, &mut |expr, _| {
            if let Expr::Call(name, _) = expr {
                names.push(name.clone());
            }
        });
        // `for_each_expr` sees the arguments of a call statement, not its
        // name.
        let mut statement_calls = Vec::new();
        for_each_block(&mut copy, &mut |body| {
            for statement in body.iter() {
                if let Stmt::Call(name, _) = statement {
                    statement_calls.push(name.clone());
                }
            }
        });
        names.extend(statement_calls);
        names.iter().all(|name| match name.split_once('.') {
            Some((other, function)) => {
                module
                    .imports
                    .iter()
                    .any(|import| matches!(import, Import::Module(m) if *m == other))
                    && defined(other, function)
            }
            None => {
                defined(module.name, name)
                    || module.imports.iter().any(|import| match import {
                        Import::Names(other, listed) => {
                            listed.contains(name) && defined(other, name)
                        }
                        Import::Module(_) => false,
                    })
            }
        })
    })
}

fn smaller_programs(program: &Program) -> Vec<Program> {
    let mut candidates = Vec::new();
    for (module_index, module) in program.modules.iter().enumerate() {
        for index in 0..module.functions.len() {
            if module.name == MAIN.name && index + 1 == module.functions.len() {
                continue;
            }
            let mut candidate = program.clone();
            candidate.modules[module_index].functions.remove(index);
            candidates.push(candidate);
        }
        for index in 0..module.imports.len() {
            let mut candidate = program.clone();
            candidate.modules[module_index].imports.remove(index);
            candidates.push(candidate);
        }
        if module.name != MAIN.name {
            let mut candidate = program.clone();
            candidate.modules.remove(module_index);
            for other in &mut candidate.modules {
                other.imports.retain(|import| match import {
                    Import::Module(name) | Import::Names(name, _) => *name != module.name,
                });
            }
            candidates.push(candidate);
        }
    }

    let mut statements = 0;
    for_each_block(&mut program.clone(), &mut |body| statements += body.len());
    for target in 0..statements {
        for mode in 0..4 {
            let mut candidate = program.clone();
            let mut seen = 0;
            let mut changed = None;
            for_each_block(&mut candidate, &mut |body| {
                if changed.is_some() || target >= seen + body.len() {
                    seen += body.len();
                    return;
                }
                let at = target - seen;
                seen = usize::MAX / 2;
                changed = Some(match (mode, &mut body[at]) {
                    (0, _) => {
                        body.remove(at);
                        true
                    }
                    // Replace a block statement with its first body.
                    (1, Stmt::If(branches, _)) => {
                        let inner = branches[0].1.clone();
                        body.splice(at..=at, inner);
                        true
                    }
                    (
                        1,
                        Stmt::While { body: inner, .. }
                        | Stmt::For { body: inner, .. }
                        | Stmt::ForEach { body: inner, .. },
                    ) => {
                        let inner = inner.clone();
                        body.splice(at..=at, inner);
                        true
                    }
                    (2, Stmt::If(_, else_branch)) => else_branch.take().is_some(),
                    (3, Stmt::If(branches, _)) if branches.len() > 1 => {
                        branches.pop();
                        true
                    }
                    _ => false,
                });
            });
            if changed == Some(true) {
                candidates.push(candidate);
            }
        }
    }

    let mut conditions = 0;
    for_each_cond(&mut program.clone(), &mut |_| conditions += 1);
    for target in 0..conditions {
        for mode in 0..3 {
            let mut candidate = program.clone();
            let mut seen = 0;
            let mut changed = false;
            for_each_cond(&mut candidate, &mut |c| {
                if seen == target {
                    let replacement = match (mode, &*c) {
                        (0, Cond::Not(inner, _)) => Some((**inner).clone()),
                        (0 | 1, Cond::And(left, right) | Cond::Or(left, right)) => {
                            Some(if mode == 0 {
                                (**left).clone()
                            } else {
                                (**right).clone()
                            })
                        }
                        (2, Cond::Cmp(..) | Cond::Truthy(_)) => None,
                        (2, _) => Some(Cond::Truthy(Expr::Lit(1))),
                        _ => None,
                    };
                    if let Some(replacement) = replacement {
                        *c = replacement;
                        changed = true;
                    }
                }
                seen += 1;
            });
            if changed {
                candidates.push(candidate);
            }
        }
    }

    let mut expressions = 0;
    for_each_expr(&mut program.clone(), &mut |_, _| expressions += 1);
    for target in 0..expressions {
        for mode in 0..4 {
            let mut candidate = program.clone();
            let mut seen = 0;
            let mut changed = false;
            for_each_expr(&mut candidate, &mut |e, bound| {
                if seen == target {
                    // Each replacement is smaller: 0 before 1 before
                    // anything else, so shrinking can't go in circles.
                    let replacement = match (mode, &*e) {
                        (0, Expr::Lit(0)) | (1, Expr::Lit(0 | 1)) => None,
                        (0, _) => Some(Expr::Lit(0)),
                        (1, _) => Some(Expr::Lit(1)),
                        (_, _) if bound => None,
                        (2, Expr::Neg(inner)) => Some((**inner).clone()),
                        (2, Expr::Bin(_, left, _)) => Some((**left).clone()),
                        (3, Expr::Bin(_, _, right)) => Some((**right).clone()),
                        _ => None,
                    };
                    if let Some(replacement) = replacement {
                        *e = replacement;
                        changed = true;
                    }
                }
                seen += 1;
            });
            if changed {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

/// What kind of failure an outcome is, so shrinking keeps the same one:
/// the line of the first diagnostic, or the first line of the error,
/// without numbers and temporary paths.
fn failure_kind(actual: &Result<Outcome, String>, expected: &Outcome) -> Option<String> {
    match actual {
        Ok(outcome) if outcome == expected => None,
        Ok(_) => Some("different result".to_string()),
        Err(error) => {
            let line = error
                .lines()
                .find(|line| line.contains("error["))
                .or_else(|| error.lines().next())
                .unwrap_or_default();
            let words: Vec<&str> = line
                .split_whitespace()
                .filter(|word| !word.contains("/tmp/") && !word.contains(".cbl"))
                .collect();
            Some(
                words
                    .join(" ")
                    .chars()
                    .filter(|c| !c.is_ascii_digit())
                    .collect(),
            )
        }
    }
}

/// Removes parts of a failing program while it keeps failing the same way.
/// Each round starts where the last one found a smaller program, since the
/// candidates before it failed recently.
fn shrink(python: &mut Python, program: Program, kind: &str) -> Program {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut current = program;
    let mut start = 0;
    'rounds: while std::time::Instant::now() < deadline {
        let mut candidates = shrink_candidates(&current);
        let rotate = start.min(candidates.len());
        candidates.rotate_left(rotate);
        for (chunk_index, chunk) in candidates.chunks(32).enumerate() {
            for (index, (candidate, expected)) in chunk.iter().zip(python.run(chunk)).enumerate() {
                let Ok(expected) = expected else {
                    continue;
                };
                let actual = run_cobble(candidate);
                if failure_kind(&actual, &expected).as_deref() == Some(kind) {
                    current = candidate.clone();
                    start = rotate + chunk_index * 32 + index;
                    continue 'rounds;
                }
            }
        }
        break;
    }
    current
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

const PYTHON_RUNNER: &str = r#"
import json, sys, types

RUNTIME = '''
def W(x):
    x &= 0xFFFFFFFF
    return x - 0x100000000 if x & 0x80000000 else x
def add(a, b): return W(a + b)
def sub(a, b): return W(a - b)
def mul(a, b): return W(a * b)
def fdiv(a, b): return W(a // b)
def fmod(a, b): return W(a % b)
def pw(a, b): return 0 if b < 0 else W(pow(a, b, 0x100000000))
def neg(a): return W(-a)
LOG = []
'''

def run(program):
    before = set(sys.modules)
    try:
        runtime = types.ModuleType("cobble_runtime")
        exec(RUNTIME, runtime.__dict__)
        sys.modules["cobble_runtime"] = runtime
        spaces = {}
        for name, source in program["modules"]:
            module = types.ModuleType(name)
            sys.modules[name] = module
            exec("from cobble_runtime import *\n" + source, module.__dict__)
            spaces[name] = module.__dict__
        value = spaces["main"]["main"]()
        variables = {var: spaces[name].get(var) for name, var in program["vars"]}
        return {"return": value, "vars": variables, "log": runtime.LOG}
    except Exception as error:
        return {"error": f"{type(error).__name__}: {error}"}
    finally:
        for name in set(sys.modules) - before:
            del sys.modules[name]

# One JSON list of programs per line in, one list of results per line out.
for line in sys.stdin:
    print(json.dumps([run(program) for program in json.loads(line)]), flush=True)
"#;

/// A module variable's value at the end of a run.
#[derive(Debug, PartialEq)]
enum Value {
    Int(i64),
    Str(String),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
    Missing,
}

impl Value {
    fn from_json(value: Option<&serde_json::Value>) -> Value {
        match value {
            Some(serde_json::Value::Number(n)) => Value::Int(n.as_f64().unwrap() as i64),
            Some(serde_json::Value::String(s)) => Value::Str(s.clone()),
            Some(serde_json::Value::Array(items)) => Value::List(
                items
                    .iter()
                    .map(|item| Value::from_json(Some(item)))
                    .collect(),
            ),
            Some(serde_json::Value::Object(fields)) => Value::Map(
                fields
                    .iter()
                    .map(|(name, value)| (name.clone(), Value::from_json(Some(value))))
                    .collect(),
            ),
            _ => Value::Missing,
        }
    }
}

#[derive(Debug, PartialEq)]
struct Outcome {
    returned: Option<i32>,
    vars: BTreeMap<String, Value>,
    log: Vec<String>,
}

/// Each module variable with its module and whether a score holds it.
fn program_vars(program: &Program) -> Vec<(&'static str, String, bool)> {
    program
        .modules
        .iter()
        .flat_map(|module| {
            module
                .vars
                .iter()
                .map(|(name, value)| (module.name, name.clone(), matches!(value, Val::Int(_))))
        })
        .collect()
}

/// One `python3` process that runs the generated programs in batches.
struct Python {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _dir: tempfile::TempDir,
}

impl Python {
    /// `None` when `python3` can't be started.
    fn start() -> Option<Python> {
        let dir = tempfile::TempDir::new().unwrap();
        let runner = dir.path().join("runner.py");
        std::fs::write(&runner, PYTHON_RUNNER).unwrap();
        let mut child = Command::new("python3")
            .arg("-I")
            .arg(&runner)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Some(Python {
            child,
            stdin,
            stdout,
            _dir: dir,
        })
    }

    fn run(&mut self, programs: &[Program]) -> Vec<Result<Outcome, String>> {
        let input: Vec<serde_json::Value> = programs
            .iter()
            .map(|program| {
                let modules: Vec<(&str, String)> = program
                    .modules
                    .iter()
                    .map(|module| (module.name, print_module(module, Language::Python)))
                    .collect();
                let vars: Vec<(&str, String)> = program_vars(program)
                    .into_iter()
                    .map(|(module, name, _)| (module, name))
                    .collect();
                serde_json::json!({ "modules": modules, "vars": vars })
            })
            .collect();
        writeln!(self.stdin, "{}", serde_json::to_string(&input).unwrap()).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        assert!(
            !line.is_empty(),
            "python3 stopped while running the programs"
        );
        let results: Vec<serde_json::Value> = serde_json::from_str(&line).unwrap();
        results.into_iter().map(python_outcome).collect()
    }
}

impl Drop for Python {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn python_outcome(result: serde_json::Value) -> Result<Outcome, String> {
    if let Some(error) = result.get("error") {
        return Err(error.as_str().unwrap().to_string());
    }
    let vars = result["vars"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| (name.clone(), Value::from_json(Some(value))))
        .collect();
    Ok(Outcome {
        returned: result["return"].as_i64().map(|v| v as i32),
        vars,
        log: result["log"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| line.as_str().unwrap().to_string())
            .collect(),
    })
}

fn run_cobble(program: &Program) -> Result<Outcome, String> {
    let files: Vec<(String, String)> = program
        .modules
        .iter()
        .map(|module| {
            (
                format!("{}.cbl", module.name),
                print_module(module, Language::Cobble),
            )
        })
        .collect();
    let loaded = match files.as_slice() {
        [(_, source)] => Sim::try_load(source, &[]),
        files => {
            let files: Vec<(&str, &str)> = files
                .iter()
                .map(|(name, source)| (name.as_str(), source.as_str()))
                .collect();
            Sim::try_load_project(&files, &[])
        }
    };
    let mut sim = loaded.map_err(|error| format!("build failed: {error}"))?;
    let returned =
        panic::catch_unwind(AssertUnwindSafe(|| sim.call("main"))).map_err(|payload| {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            format!("simulator panicked: {message}")
        })?;
    let vars = program_vars(program)
        .into_iter()
        .map(|(_, name, score)| {
            let value = if score {
                sim.try_score(&name, "sim.vars")
                    .map_or(Value::Missing, |v| Value::Int(v.into()))
            } else {
                Value::from_json(sim.storage.get("vars").and_then(|vars| vars.get(&name)))
            };
            (name, value)
        })
        .collect();
    Ok(Outcome {
        returned,
        vars,
        log: sim
            .log
            .iter()
            .map(|line| line.strip_prefix("Server: ").unwrap_or(line).to_string())
            .collect(),
    })
}

fn env_number(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be a number, got `{value}`")),
        Err(_) => default,
    }
}

#[test]
fn random_programs_match_python() {
    let Some(mut python) = Python::start() else {
        assert!(
            std::env::var("COBBLE_DIFF_REQUIRE_PYTHON").as_deref() != Ok("1"),
            "python3 is required for the differential test"
        );
        eprintln!("skipping the differential test: python3 not found");
        return;
    };
    let cases = env_number("COBBLE_DIFF_CASES", 300);
    let first_seed = env_number("COBBLE_DIFF_SEED", 1);

    let (seeds, programs): (Vec<u64>, Vec<Program>) = (0..cases)
        .map(|index| {
            let seed = first_seed + index;
            let mut generator = Generator {
                rng: Rng(seed),
                counter: 0,
                functions: 0,
                consts: Vec::new(),
            };
            (seed, generator.program())
        })
        .unzip();
    let expected: Vec<Outcome> = python
        .run(&programs)
        .into_iter()
        .zip(seeds.iter().zip(&programs))
        .map(|(expected, (seed, program))| {
            expected.unwrap_or_else(|error| {
                panic!(
                    "seed {seed}: the generated Python failed ({error}):\n{}",
                    print_program(program, Language::Python)
                )
            })
        })
        .collect();

    // The simulator panics on commands Minecraft would refuse; those
    // panics are failures, reported below.
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let mut failures = Vec::new();
    for ((seed, program), expected) in seeds.iter().zip(&programs).zip(&expected) {
        let actual = run_cobble(program);
        if let Some(kind) = failure_kind(&actual, expected) {
            failures.push((*seed, program, kind));
        }
    }
    let shrinking = std::env::var("COBBLE_DIFF_SHRINK").as_deref() != Ok("0");
    let first_report = failures.first().map(|(seed, program, kind)| {
        let small = if shrinking {
            shrink(&mut python, (*program).clone(), kind)
        } else {
            (*program).clone()
        };
        let expected = python.run(std::slice::from_ref(&small)).remove(0);
        let actual = run_cobble(&small);
        let label = if shrinking { "shrunk" } else { "as generated" };
        format!(
            "seed {seed}, {label}:\n--- cobble ---\n{}--- python ---\n{}\
             --- python result ---\n{expected:?}\n--- cobble result ---\n{actual:?}\n",
            print_program(&small, Language::Cobble),
            print_program(&small, Language::Python)
        )
    });
    panic::set_hook(default_hook);

    let summaries: Vec<String> = failures
        .iter()
        .map(|(seed, _, kind)| format!("seed {seed}: {kind}"))
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {cases} programs differ from Python. Rerun one with \
         COBBLE_DIFF_SEED=<seed> COBBLE_DIFF_CASES=1.\n\n{}\n\n{}",
        failures.len(),
        summaries.join("\n"),
        first_report.unwrap_or_default()
    );
}

// --- Container shapes ---
//
// Small programs of the shapes the generator above doesn't make: an item of
// another kind added to a list by `+=` or `+` and read by a key in another
// function, slices of a parameter joined, and a number compared with a
// value that may be a list, a map, or text. Each program is the same text
// in Cobble and in Python and appends its results to the list `out`.

/// A literal of `kind` 0 (a list of numbers) or 1 (a map of numbers), with
/// the keys a read can use.
fn container_literal(rng: &mut Rng, kind: usize) -> (String, Vec<String>) {
    let size = 1 + rng.below(3);
    let values: Vec<i32> = (0..size).map(|_| rng.range(-9, 9)).collect();
    if kind == 0 {
        let keys = (0..size)
            .flat_map(|index| [index.to_string(), format!("-{}", size - index)])
            .collect();
        let items: Vec<String> = values.iter().map(i32::to_string).collect();
        (format!("[{}]", items.join(", ")), keys)
    } else {
        let names = ["a", "k", "x y"];
        let keys: Vec<String> = names[..size]
            .iter()
            .map(|name| format!("{name:?}"))
            .collect();
        let entries: Vec<String> = keys
            .iter()
            .zip(&values)
            .map(|(key, value)| format!("{key}: {value}"))
            .collect();
        (format!("{{{}}}", entries.join(", ")), keys)
    }
}

/// `rows` starts with an item of one kind; `main` adds one of the other
/// kind, and another function reads and writes it by a parameter key and by
/// a literal key.
fn added_item_program(rng: &mut Rng) -> String {
    let kind = rng.below(2);
    let (first, _) = container_literal(rng, kind);
    let (added, keys) = container_literal(rng, 1 - kind);
    let key = rng.pick(&keys);
    let literal_key = rng.pick(&keys);
    let value = rng.range(-9, 9);
    let (setup, base, path) = match rng.below(3) {
        0 => ("rows = [FIRST]", "rows", "rows[1]"),
        1 => ("rows = [[FIRST]]", "rows[0]", "rows[0][1]"),
        _ => ("rows = {\"a\": [FIRST]}", "rows[\"a\"]", "rows[\"a\"][1]"),
    };
    let add = match rng.below(3) {
        0 => format!("{base} += [{added}]"),
        1 => format!("{base} = {base} + [{added}]"),
        _ => format!("{base}.append({added})"),
    };
    let global = if base == "rows" {
        "    global rows\n"
    } else {
        ""
    };
    format!(
        "{setup}\nout = []\n\ndef use(key):\n    out.append({path}[key])\n    out.append({path}[{literal_key}])\n    {path}[key] = {value}\n    {path}[key] += 1\n\ndef main():\n{global}    {add}\n    use({key})\n    out.append(rows)\n",
        setup = setup.replace("FIRST", &first),
    )
}

/// A list or a text, with bounds of slices of it.
fn sliced_program(rng: &mut Rng) -> String {
    let size = rng.below(5);
    let value = if rng.chance(50) {
        let items: Vec<String> = (0..size).map(|_| rng.range(0, 9).to_string()).collect();
        format!("[{}]", items.join(", "))
    } else {
        let text: String = (0..size).map(|_| rng.pick(&['a', 'b', 'c'])).collect();
        format!("{text:?}")
    };
    let bound = |rng: &mut Rng| rng.range(-(size as i32) - 1, size as i32 + 1);
    let (i, j) = (bound(rng), bound(rng));
    let count = rng.range(0, 3);
    let joins = [
        "v[:i] + v[i:]",
        "v[:i] + v[i:j] + v[j:]",
        "v[i:] + v[:i]",
        "v[:i] + (v[i:j] + v[j:])",
        "a + b",
        "b + a + b",
        "v[i:j] * n",
        "a * n",
        "v[:0] + v",
    ];
    let mut body = String::from("    a = v[:i]\n    b = v[i:j]\n");
    for _ in 0..1 + rng.below(3) {
        writeln!(body, "    out.append({})", rng.pick(&joins)).unwrap();
    }
    writeln!(body, "    out.append(len(v[i:j] + v[:i]))").unwrap();
    format!(
        "out = []\n\ndef f(v, i, j, n):\n{body}\ndef main():\n    f({value}, {i}, {j}, {count})\n"
    )
}

/// A number compared with a parameter that may hold a number, text, a
/// list, or a map.
fn compared_program(rng: &mut Rng) -> String {
    let values = [
        "0",
        "1",
        "3",
        "-1",
        "[]",
        "[1]",
        "[0, 0, 0]",
        "\"\"",
        "\"a\"",
        "\"abc\"",
        "{}",
        "{\"k\": 1}",
    ];
    let number = rng.pick(&["0", "1", "3", "-1"]);
    let mut calls = String::new();
    for _ in 0..1 + rng.below(4) {
        writeln!(calls, "    out.append(f({}))", rng.pick(&values)).unwrap();
    }
    format!(
        "out = []\n\ndef f(m):\n    r = 0\n    if m == {number}:\n        r += 1\n    if {number} == m:\n        r += 2\n    if m != {number}:\n        r += 4\n    x = m == {number}\n    r += x * 8\n    return r\n\ndef main():\n{calls}"
    )
}

const SHAPE_RUNNER: &str = r#"
import json, sys
for line in sys.stdin:
    results = []
    for source in json.loads(line):
        scope = {}
        try:
            exec(source + "\nmain()\n", scope)
            results.append({"out": scope["out"]})
        except Exception as error:
            results.append({"error": repr(error)})
    print(json.dumps(results), flush=True)
"#;

#[test]
fn container_shapes_match_python() {
    let dir = tempfile::TempDir::new().unwrap();
    let runner = dir.path().join("shapes.py");
    std::fs::write(&runner, SHAPE_RUNNER).unwrap();
    let Ok(mut child) = Command::new("python3")
        .arg("-I")
        .arg(&runner)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    else {
        assert!(
            std::env::var("COBBLE_DIFF_REQUIRE_PYTHON").as_deref() != Ok("1"),
            "python3 is required for the differential test"
        );
        return;
    };
    let cases = env_number("COBBLE_DIFF_CASES", 300);
    let first_seed = env_number("COBBLE_DIFF_SEED", 1);
    let programs: Vec<(u64, String)> = (0..cases)
        .map(|index| {
            let seed = first_seed + index;
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let source = match index % 3 {
                0 => added_item_program(&mut rng),
                1 => sliced_program(&mut rng),
                _ => compared_program(&mut rng),
            };
            (seed, source)
        })
        .collect();
    let sources: Vec<&str> = programs.iter().map(|(_, source)| source.as_str()).collect();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{}", serde_json::to_string(&sources).unwrap()).unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let _ = child.kill();
    let expected: Vec<serde_json::Value> = serde_json::from_str(&line).unwrap();

    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let mut failures = Vec::new();
    let mut checked = 0;
    for ((seed, source), expected) in programs.iter().zip(&expected) {
        // Python raised: the program isn't one this test checks.
        let Some(want) = expected.get("out") else {
            continue;
        };
        checked += 1;
        let got = match Sim::try_load(source, &[]) {
            Err(error) => Err(format!("build failed: {error}")),
            Ok(mut sim) => panic::catch_unwind(AssertUnwindSafe(|| {
                sim.call("main");
                sim.storage["vars"]["out"].clone()
            }))
            .map_err(|_| "simulator panicked".to_string()),
        };
        if got.as_ref() != Ok(want) {
            failures.push(format!(
                "seed {seed}:\n{source}--- python ---\n{want}\n--- cobble ---\n{got:?}\n"
            ));
        }
    }
    panic::set_hook(default_hook);
    assert!(
        checked * 4 >= cases * 3,
        "Python raised for {} of {cases} container programs",
        cases - checked
    );
    assert!(
        failures.is_empty(),
        "{} of {cases} container programs differ from Python:\n\n{}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
