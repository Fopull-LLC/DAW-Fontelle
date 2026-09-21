//! The wavetable editor's formula (`docs/flopsynth-next.md` §4.3): a small
//! expression over the phase, `x` in 0..1 — `sin(x*2) + 0.3*saw(x)` — with
//! the waves taking **cycles** rather than radians, because a wavetable is
//! cycles and nobody drawing one wants to write `2*pi*x`.
//!
//! What it has: numbers, `x`, `pi`, `+ − * / ^`, parentheses, unary minus,
//! and the functions `sin cos tri square saw abs tanh sqrt exp`. What it
//! does not: variables, conditionals, a second argument. A formula is one
//! frame's shape; anything more is a table somebody should draw.

/// A parsed formula, evaluated per sample.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f32),
    Phase,
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Call(Func, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    /// `sin(v)`: a sine over `v` cycles.
    Sin,
    Cos,
    /// A triangle, −1 at 0, 1 at a half.
    Tri,
    /// A square, 1 for the first half.
    Square,
    /// A saw, rising from −1 to 1 over the cycle.
    Saw,
    Abs,
    Tanh,
    Sqrt,
    Exp,
}

impl Func {
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tri" => Self::Tri,
            "square" | "sqr" => Self::Square,
            "saw" => Self::Saw,
            "abs" => Self::Abs,
            "tanh" => Self::Tanh,
            "sqrt" => Self::Sqrt,
            "exp" => Self::Exp,
            _ => return None,
        })
    }

    fn apply(self, v: f32) -> f32 {
        match self {
            Self::Sin => (v * std::f32::consts::TAU).sin(),
            Self::Cos => (v * std::f32::consts::TAU).cos(),
            Self::Tri => {
                let t = v.rem_euclid(1.0);
                if t < 0.5 {
                    4.0 * t - 1.0
                } else {
                    3.0 - 4.0 * t
                }
            }
            Self::Square => {
                if v.rem_euclid(1.0) < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Self::Saw => 2.0 * v.rem_euclid(1.0) - 1.0,
            Self::Abs => v.abs(),
            Self::Tanh => v.tanh(),
            Self::Sqrt => v.max(0.0).sqrt(),
            Self::Exp => v.exp(),
        }
    }
}

impl Expr {
    /// The formula's value at phase `x`.
    pub fn eval(&self, x: f32) -> f32 {
        match self {
            Self::Number(n) => *n,
            Self::Phase => x,
            Self::Neg(a) => -a.eval(x),
            Self::Add(a, b) => a.eval(x) + b.eval(x),
            Self::Sub(a, b) => a.eval(x) - b.eval(x),
            Self::Mul(a, b) => a.eval(x) * b.eval(x),
            Self::Div(a, b) => {
                let d = b.eval(x);
                if d == 0.0 { 0.0 } else { a.eval(x) / d }
            }
            Self::Pow(a, b) => {
                let (base, power) = (a.eval(x), b.eval(x));
                // A negative base to a whole power keeps its sign, so
                // `sin(x)^3` is the cube and not a NaN.
                if base < 0.0 && power.fract() == 0.0 {
                    let magnitude = (-base).powf(power);
                    if (power as i64) % 2 == 0 {
                        magnitude
                    } else {
                        -magnitude
                    }
                } else {
                    base.powf(power)
                }
            }
            Self::Call(f, a) => f.apply(a.eval(x)),
        }
    }
}

/// Parses `text` into an expression, or says what is wrong with it.
pub fn parse(text: &str) -> Result<Expr, String> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        at: 0,
    };
    let expr = parser.expr()?;
    parser.skip_space();
    if parser.at < parser.chars.len() {
        return Err(format!(
            "unexpected '{}' at {}",
            parser.chars[parser.at],
            parser.at + 1
        ));
    }
    Ok(expr)
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn skip_space(&mut self) {
        while self.at < self.chars.len() && self.chars[self.at].is_whitespace() {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.chars.get(self.at).copied()
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut left = self.term()?;
        loop {
            match self.peek() {
                Some('+') => {
                    self.at += 1;
                    left = Expr::Add(Box::new(left), Box::new(self.term()?));
                }
                Some('-') => {
                    self.at += 1;
                    left = Expr::Sub(Box::new(left), Box::new(self.term()?));
                }
                _ => return Ok(left),
            }
        }
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut left = self.factor()?;
        loop {
            match self.peek() {
                Some('*') => {
                    self.at += 1;
                    left = Expr::Mul(Box::new(left), Box::new(self.factor()?));
                }
                Some('/') => {
                    self.at += 1;
                    left = Expr::Div(Box::new(left), Box::new(self.factor()?));
                }
                _ => return Ok(left),
            }
        }
    }

    fn factor(&mut self) -> Result<Expr, String> {
        let base = self.unary()?;
        if self.peek() == Some('^') {
            self.at += 1;
            // Right-associative: `2^3^2` is `2^9`.
            let power = self.factor()?;
            return Ok(Expr::Pow(Box::new(base), Box::new(power)));
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.peek() == Some('-') {
            self.at += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.peek() == Some('+') {
            self.at += 1;
            return self.unary();
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let Some(c) = self.peek() else {
            return Err("the formula ends early".to_string());
        };
        if c == '(' {
            self.at += 1;
            let inner = self.expr()?;
            if self.peek() != Some(')') {
                return Err("a '(' with no ')'".to_string());
            }
            self.at += 1;
            return Ok(inner);
        }
        if c.is_ascii_digit() || c == '.' {
            let start = self.at;
            while self.at < self.chars.len()
                && (self.chars[self.at].is_ascii_digit() || self.chars[self.at] == '.')
            {
                self.at += 1;
            }
            let text: String = self.chars[start..self.at].iter().collect();
            return text
                .parse::<f32>()
                .map(Expr::Number)
                .map_err(|_| format!("'{text}' is not a number"));
        }
        if c.is_ascii_alphabetic() {
            let start = self.at;
            while self.at < self.chars.len() && self.chars[self.at].is_ascii_alphanumeric() {
                self.at += 1;
            }
            let name: String = self.chars[start..self.at].iter().collect();
            return match name.as_str() {
                "x" => Ok(Expr::Phase),
                "pi" => Ok(Expr::Number(std::f32::consts::PI)),
                _ => {
                    let Some(func) = Func::named(&name) else {
                        return Err(format!("no function called '{name}'"));
                    };
                    if self.peek() != Some('(') {
                        return Err(format!("{name} needs a '('"));
                    }
                    self.at += 1;
                    let inner = self.expr()?;
                    if self.peek() != Some(')') {
                        return Err(format!("{name}( with no ')'"));
                    }
                    self.at += 1;
                    Ok(Expr::Call(func, Box::new(inner)))
                }
            };
        }
        Err(format!("unexpected '{c}' at {}", self.at + 1))
    }
}
