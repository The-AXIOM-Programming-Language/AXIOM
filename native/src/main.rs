pub mod eeg;
pub mod edf;
pub mod edf_data;
pub mod units;
pub mod simulation;
pub mod language;

use std::{collections::HashMap, env, fs, process};
use units::{Quantity, Unit, UnitError};

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64), String(String), Ident(String),
    Plus, Minus, Star, Slash, Equal, LParen, RParen, Newline, Eof,
}

#[derive(Debug, Clone)]
struct LexError { line: usize, message: String }

fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = source.chars().collect();
    let (mut i, mut line) = (0, 1);
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c == ' ' || c == '\t' || c == '\r' { i += 1; continue; }
        if c == '\n' { out.push(Token::Newline); line += 1; i += 1; continue; }
        if c == '#' { while i < chars.len() && chars[i] != '\n' { i += 1; } continue; }
        if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i+1].is_ascii_digit()) {
            let start = i; i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { i += 1; }
            let raw: String = chars[start..i].iter().collect();
            let value = raw.parse::<f64>().map_err(|_| LexError { line, message: format!("Invalid number '{raw}'") })?;
            if !value.is_finite() { return Err(LexError { line, message: "Numeric literal must be finite".into() }); }
            out.push(Token::Number(value)); continue;
        }
        if c == '"' {
            i += 1; let start = i;
            while i < chars.len() && chars[i] != '"' && chars[i] != '\n' { i += 1; }
            if i >= chars.len() || chars[i] != '"' { return Err(LexError { line, message: "Unterminated string literal".into() }); }
            out.push(Token::String(chars[start..i].iter().collect())); i += 1; continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i; i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') { i += 1; }
            out.push(Token::Ident(chars[start..i].iter().collect())); continue;
        }
        let token = match c {
            '+' => Token::Plus, '-' => Token::Minus, '*' => Token::Star, '/' => Token::Slash,
            '=' => Token::Equal, '(' => Token::LParen, ')' => Token::RParen,
            _ => return Err(LexError { line, message: format!("Unexpected character '{c}'") }),
        };
        out.push(token); i += 1;
    }
    out.push(Token::Eof);
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
enum Value { Quantity(Quantity), Text(String) }

#[derive(Debug, Clone)]
enum Expr { Value(Value), Name(String), Unary(bool, Box<Expr>), Binary(Box<Expr>, char, Box<Expr>) }

#[derive(Debug, Clone)]
enum Stmt { Let(String, Expr), Show(Expr) }

struct Parser { tokens: Vec<Token>, pos: usize }
impl Parser {
    fn current(&self) -> &Token { &self.tokens[self.pos] }
    fn advance(&mut self) -> Token { let t = self.tokens[self.pos].clone(); self.pos += 1; t }
    fn consume_newlines(&mut self) { while *self.current() == Token::Newline { self.advance(); } }
    fn parse(&mut self) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        self.consume_newlines();
        while *self.current() != Token::Eof {
            let stmt = match self.advance() {
                Token::Ident(word) if word == "let" => {
                    let name = match self.advance() { Token::Ident(n) => n, _ => return Err("Expected a variable name after 'let'".into()) };
                    if self.advance() != Token::Equal { return Err("Expected '=' in variable declaration".into()); }
                    Stmt::Let(name, self.expression(0)?)
                }
                Token::Ident(word) if word == "show" => Stmt::Show(self.expression(0)?),
                Token::Ident(word) => return Err(format!("Unknown statement '{word}'")),
                _ => return Err("Expected 'let' or 'show' at the start of a statement".into()),
            };
            statements.push(stmt);
            if *self.current() != Token::Newline && *self.current() != Token::Eof {
                return Err("Expected the end of the line after the statement".into());
            }
            self.consume_newlines();
        }
        Ok(statements)
    }
    fn expression(&mut self, min_bp: u8) -> Result<Expr, String> {
        let token = self.advance();
        let mut lhs = match token {
            Token::Number(n) => Expr::Value(Value::Quantity(Quantity::scalar(n).map_err(|e| e.to_string())?)),
            Token::String(s) => Expr::Value(Value::Text(s)),
            Token::Ident(n) => Expr::Name(n),
            Token::Minus => Expr::Unary(true, Box::new(self.expression(9)?)),
            Token::Plus => Expr::Unary(false, Box::new(self.expression(9)?)),
            Token::LParen => { let e = self.expression(0)?; if self.advance() != Token::RParen { return Err("Expected ')'".into()); } e },
            _ => return Err("Expected an expression".into()),
        };
        loop {
            // A space between a magnitude and unit is part of AXIOM syntax: 20 ms.
            if let Token::Ident(name) = self.current() {
                if Unit::lookup(name).is_some() {
                    if 3 < min_bp { break; }
                    let unit_name = name.clone();
                    self.advance();
                    lhs = Expr::Binary(Box::new(lhs), '*', Box::new(Expr::Name(unit_name)));
                    continue;
                }
            }
            let (op, left_bp, right_bp) = match self.current() {
                Token::Plus => ('+', 1, 2), Token::Minus => ('-', 1, 2),
                Token::Star => ('*', 3, 4), Token::Slash => ('/', 3, 4),
                _ => break,
            };
            if left_bp < min_bp { break; }
            self.advance();
            let rhs = self.expression(right_bp)?;
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        Ok(lhs)
    }
}

fn unit_value(name: &str) -> Option<Quantity> {
    let unit = Unit::lookup(name)?;
    Quantity::from_unit(1.0, unit).ok()
}

fn arithmetic_error(error: UnitError) -> String { error.to_string() }

fn eval(expr: &Expr, vars: &HashMap<String, Value>) -> Result<Value, String> {
    match expr {
        Expr::Value(v) => Ok(v.clone()),
        Expr::Name(n) => {
            if let Some(v) = vars.get(n) { return Ok(v.clone()); }
            if let Some(quantity) = unit_value(n) { return Ok(Value::Quantity(quantity)); }
            Err(format!("Unknown name '{n}'"))
        }
        Expr::Unary(negative, e) => match eval(e, vars)? {
            Value::Quantity(q) => {
                let magnitude = if *negative { -q.magnitude } else { q.magnitude };
                Ok(Value::Quantity(Quantity::new(magnitude, q.dimension).map_err(arithmetic_error)?))
            }
            Value::Text(_) => Err("Unary arithmetic requires a number or quantity".into()),
        },
        Expr::Binary(a, op, b) => {
            let left = eval(a, vars)?; let right = eval(b, vars)?;
            match (left, right) {
                (Value::Quantity(x), Value::Quantity(y)) => {
                    let result = match op {
                        '+' => x.add(y),
                        '-' => x.sub(y),
                        '*' => x.mul(y),
                        '/' => x.div(y),
                        _ => unreachable!(),
                    }.map_err(arithmetic_error)?;
                    Ok(Value::Quantity(result))
                }
                _ => Err("This operator requires numeric or quantity operands".into()),
            }
        }
    }
}

fn execute(source: &str) -> Result<Vec<String>, String> {
    let tokens = lex(source).map_err(|e| format!("Line {}: {}", e.line, e.message))?;
    let mut parser = Parser { tokens, pos: 0 };
    let statements = parser.parse()?;
    let mut vars = HashMap::new();
    let mut output = Vec::new();
    for stmt in statements {
        match stmt {
            Stmt::Let(name, expr) => {
                if ["let", "show"].contains(&name.as_str()) { return Err(format!("'{name}' is reserved")); }
                let value = eval(&expr, &vars)?;
                vars.insert(name, value);
            }
            Stmt::Show(expr) => output.push(match eval(&expr, &vars)? {
                Value::Quantity(q) => q.magnitude.to_string(),
                Value::Text(s) => s,
            }),
        }
    }
    Ok(output)
}

fn print_help() {
    println!("AXIOM — scientific programming language");
    println!("\nUSAGE:");
    println!("  axiom <COMMAND> [OPTIONS]");
    println!("\nCOMMANDS:");
    println!("  run <FILE.ax>       Execute an AXIOM program");
    println!("  check <FILE.ax>     Validate AXIOM source syntax");
    println!("  new <NAME>          Create a starter AXIOM program");
    println!("  doctor              Check the native toolchain environment");
    println!("  build <FILE.ax>     Check source and report backend status");
    println!("  help                Show this help");
    println!("\nOPTIONS:");
    println!("  -h, --help          Show help");
    println!("  -V, --version       Show version");
}
fn read_ax_file(path: &str) -> Result<String, String> {
    if !path.ends_with(".ax") { return Err("source files must use the .ax extension".into()); }
    fs::read_to_string(path).map_err(|e| format!("cannot read '{path}': {e}"))
}
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args[0] == "help" || args[0] == "--help" || args[0] == "-h" {
        print_help(); return;
    }
    if args[0] == "--version" || args[0] == "-V" || args[0] == "version" {
        println!("AXIOM {}", env!("CARGO_PKG_VERSION")); return;
    }
    let fail = |message: String, code: i32| -> ! {
        eprintln!("error: {message}");
        process::exit(code);
    };
    match args[0].as_str() {
        "check" | "run" | "build" => {
            if args.len() != 2 { fail(format!("usage: axiom {} <FILE.ax>", args[0]), 2); }
            let path = &args[1];
            let source = read_ax_file(path).unwrap_or_else(|e| fail(e, 2));
            match language::check_source(&source) {
                Ok(()) => {},
                Err(e) => fail(format!("{path}: {e}"), 1),
            }
            match args[0].as_str() {
                "check" => println!("✓ {path}: syntax OK"),
                "run" => match language::run_source(&source) {
                    Ok(lines) => for line in lines { println!("{line}"); },
                    Err(e) => fail(format!("{path}: {e}"), 1),
                },
                "build" => {
                    println!("✓ {path}: source validation passed");
                    println!("AXIOM native code generation is not available yet.");
                    println!("Use 'axiom run {path}' to execute with the native interpreter.");
                },
                _ => unreachable!(),
            }
        }
        "new" => {
            if args.len() != 2 { fail("usage: axiom new <NAME>".into(), 2); }
            let name = &args[1];
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                fail("project name may contain only letters, numbers, '-' and '_'".into(), 2);
            }
            let path = format!("{name}.ax");
            if Path::new(&path).exists() { fail(format!("'{path}' already exists"), 1); }
            let template = format!("# {name}\n# An AXIOM computational neuroscience experiment\n\nneuron \"{name}\":\n    membrane_time = 20 ms\n    resting_voltage = -65 mV\n    spike_threshold = -50 mV\n    reset_voltage = -65 mV\n    input_resistance = 10 MOhm\n    initial_voltage = -65 mV\n\ninject current 2 nA\nsimulate for 100 ms using time_step 0.1 ms using solver rk4\n\nshow spike_count\nshow spike_times\nsave results to \"{name}_results.csv\"\n");
            fs::write(&path, template).unwrap_or_else(|e| fail(format!("cannot create '{path}': {e}"), 2));
            println!("Created {path}");
            println!("Next: axiom check {path}");
        }
        "doctor" => {
            println!("AXIOM {}", env!("CARGO_PKG_VERSION"));
            println!("Native Rust runtime: available");
            println!("EEG/EDF modules: included in this build");
            println!("Native machine-code backend: not implemented");
        }
        other => fail(format!("unknown command '{other}'. Run 'axiom help'."), 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn evaluates_arithmetic_and_units() {
        let out = execute("let duration = 20 ms\nshow duration * 2\n").unwrap();
        assert_eq!(out, vec!["40"]);
    }
    #[test] fn respects_precedence() {
        assert_eq!(execute("show 2 + 3 * 4").unwrap(), vec!["14"]);
    }
    #[test] fn handles_strings() {
        assert_eq!(execute("show \"AXIOM\"").unwrap(), vec!["AXIOM"]);
    }
    #[test] fn reports_unknown_names() {
        assert!(execute("show missing").unwrap_err().contains("Unknown name"));
    }
    #[test] fn rejects_division_by_zero() {
        assert!(execute("show 1 / 0").unwrap_err().contains("division by zero"));
    }
    #[test] fn rejects_adding_time_to_voltage() {
        let error = execute("show 2 ms + 3 mV").unwrap_err();
        assert!(error.contains("incompatible dimensions"));
    }
    #[test] fn permits_ohms_times_current_to_produce_voltage() {
        assert_eq!(execute("show 10 MOhm * 2 nA").unwrap(), vec!["20"]);
    }
}
