mod eeg;

use std::{env, fs, process};

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
enum Value { Number(f64), Text(String) }

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
            Token::Number(n) => Expr::Value(Value::Number(n)),
            Token::String(s) => Expr::Value(Value::Text(s)),
            Token::Ident(n) => Expr::Name(n),
            Token::Minus => Expr::Unary(true, Box::new(self.expression(9)?)),
            Token::Plus => Expr::Unary(false, Box::new(self.expression(9)?)),
            Token::LParen => { let e = self.expression(0)?; if self.advance() != Token::RParen { return Err("Expected ')'".into()); } e },
            _ => return Err("Expected an expression".into()),
        };
        loop {
            // Scientific literals permit a space between a magnitude and unit:
            // "20 ms" is parsed as the multiplication 20 * ms.
            if let Token::Ident(name) = self.current() {
                if unit_scale(name).is_some() {
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

fn unit_scale(name: &str) -> Option<f64> {
    Some(match name {
        "ms" => 1.0, "s" => 1000.0, "us" => 0.001,
        "mV" => 1.0, "V" => 1000.0,
        "nA" => 1.0, "pA" => 0.001,
        "MOhm" => 1.0, "nF" => 1.0, "Hz" => 1.0,
        _ => return None,
    })
}
fn eval(expr: &Expr, vars: &std::collections::HashMap<String, Value>) -> Result<Value, String> {
    match expr {
        Expr::Value(v) => Ok(v.clone()),
        Expr::Name(n) => {
            if let Some(v) = vars.get(n) { return Ok(v.clone()); }
            if let Some(scale) = unit_scale(n) { return Ok(Value::Number(scale)); }
            Err(format!("Unknown name '{n}'"))
        }
        Expr::Unary(negative, e) => match eval(e, vars)? {
            Value::Number(n) => Ok(Value::Number(if *negative { -n } else { n })),
            _ => Err("Unary arithmetic requires a number".into()),
        },
        Expr::Binary(a, op, b) => {
            let left = eval(a, vars)?; let right = eval(b, vars)?;
            match (left, right) {
                (Value::Number(x), Value::Number(y)) => {
                    let result = match op { '+' => x+y, '-' => x-y, '*' => x*y, '/' if y != 0.0 => x/y, '/' => return Err("Division by zero".into()), _ => unreachable!() };
                    if !result.is_finite() { return Err("Arithmetic produced a non-finite number".into()); }
                    Ok(Value::Number(result))
                }
                _ => Err("This operator requires numeric operands".into()),
            }
        }
    }
}

fn execute(source: &str) -> Result<Vec<String>, String> {
    let tokens = lex(source).map_err(|e| format!("Line {}: {}", e.line, e.message))?;
    let mut parser = Parser { tokens, pos: 0 };
    let statements = parser.parse()?;
    let mut vars = std::collections::HashMap::new();
    let mut output = Vec::new();
    for stmt in statements {
        match stmt {
            Stmt::Let(name, expr) => {
                if ["let", "show"].contains(&name.as_str()) { return Err(format!("'{name}' is reserved")); }
                let value = eval(&expr, &vars)?;
                vars.insert(name, value);
            }
            Stmt::Show(expr) => output.push(match eval(&expr, &vars)? {
                Value::Number(n) => n.to_string(), Value::Text(s) => s,
            }),
        }
    }
    Ok(output)
}

fn usage() {
    eprintln!("AXIOM native compiler v0.2.0\nUsage:\n  axiom run <file.ax>\n  axiom check <file.ax>\n  axiom --version");
}
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && args[1] == "--version" { println!("AXIOM 0.2.0 (native Rust toolchain)"); return; }
    if args.len() != 3 { usage(); process::exit(2); }
    let command = &args[1];
    let path = &args[2];
    if !path.ends_with(".ax") { eprintln!("AXIOM: source files must use the .ax extension"); process::exit(2); }
    let source = fs::read_to_string(path).unwrap_or_else(|e| { eprintln!("AXIOM: cannot read '{path}': {e}"); process::exit(2) });
    match command.as_str() {
        "check" => match lex(&source).map_err(|e| format!("Line {}: {}", e.line, e.message)).and_then(|tokens| Parser { tokens, pos: 0 }.parse().map(|_| ())) {
            Ok(()) => println!("{}: syntax OK", path),
            Err(e) => { eprintln!("AXIOM: {e}"); process::exit(1); }
        },
        "run" => match execute(&source) {
            Ok(lines) => for line in lines { println!("{line}"); },
            Err(e) => { eprintln!("AXIOM: {e}"); process::exit(1); }
        },
        _ => { usage(); process::exit(2); }
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
        assert!(execute("show 1 / 0").unwrap_err().contains("Division by zero"));
    }
}
