use crate::{CadError, Dimension, Document, Expr, Result};
use std::collections::BTreeSet;
#[derive(Clone, Copy, Debug)]
pub struct Quantity {
    pub value: f64,
    pub length: i32,
    pub angle: i32,
}
impl Quantity {
    fn scalar(v: f64) -> Self {
        Self {
            value: v,
            length: 0,
            angle: 0,
        }
    }
    pub fn matches(&self, d: Dimension) -> bool {
        match d {
            Dimension::Scalar => self.length == 0 && self.angle == 0,
            Dimension::Length => self.length == 1 && self.angle == 0,
            Dimension::Angle => self.length == 0 && self.angle == 1,
        }
    }
}
#[derive(Clone, Debug)]
enum Token {
    Number(f64),
    Name(String),
    Op(char),
    End,
}
fn tokenize(s: &str) -> Result<Vec<Token>> {
    let chars: Vec<_> = s.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '@' {
            let start = i + 1;
            i += 1;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '-' | '.' | ':'))
            {
                i += 1;
            }
            if i == start {
                return Err(CadError::Invalid(
                    "Expected a parameter identity after @".into(),
                ));
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit()
            || c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || chars[i] == '.'
                    || chars[i] == 'e'
                    || chars[i] == 'E'
                    || (chars[i] == '+' || chars[i] == '-')
                        && (chars[i - 1] == 'e' || chars[i - 1] == 'E'))
            {
                i += 1;
            }
            out.push(Token::Number(
                chars[start..i]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .map_err(|_| CadError::Invalid("Invalid number".into()))?,
            ));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '.' | ':'))
            {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else if "+-*/^(),".contains(c) {
            out.push(Token::Op(c));
            i += 1;
        } else {
            return Err(CadError::Invalid(format!(
                "Unexpected expression character {c}"
            )));
        }
    }
    out.push(Token::End);
    Ok(out)
}
fn unit(name: &str) -> Option<Quantity> {
    let (value, length, angle) = match name {
        "mm" => (1., 1, 0),
        "cm" => (10., 1, 0),
        "m" => (1000., 1, 0),
        "in" => (25.4, 1, 0),
        "ft" => (304.8, 1, 0),
        "deg" => (std::f64::consts::PI / 180., 0, 1),
        "rad" => (1., 0, 1),
        "pi" => (std::f64::consts::PI, 0, 0),
        _ => return None,
    };
    Some(Quantity {
        value,
        length,
        angle,
    })
}
struct Parser<'a, F: FnMut(&str) -> Result<Quantity>> {
    tokens: Vec<Token>,
    index: usize,
    resolve: &'a mut F,
}
impl<F: FnMut(&str) -> Result<Quantity>> Parser<'_, F> {
    fn parse(&mut self, min: u8) -> Result<Quantity> {
        let token = self.tokens[self.index].clone();
        self.index += 1;
        let mut left = match token {
            Token::Number(n) => {
                let mut q = Quantity::scalar(n);
                if let Token::Name(n) = &self.tokens[self.index]
                    && let Some(u) = unit(n)
                {
                    q = mul(q, u, 1);
                    self.index += 1;
                }
                q
            }
            Token::Name(n) => {
                if matches!(self.tokens[self.index], Token::Op('(')) {
                    self.index += 1;
                    let q = self.parse(0)?;
                    if !matches!(self.tokens[self.index], Token::Op(')')) {
                        return Err(CadError::Invalid("Expected closing parenthesis".into()));
                    }
                    self.index += 1;
                    match n.as_str() {
                        "sin" | "cos" | "tan" => {
                            if q.length != 0 || !matches!(q.angle, 0 | 1) {
                                return Err(CadError::Invalid(
                                    "Trigonometry requires an angle".into(),
                                ));
                            }
                            Quantity::scalar(match n.as_str() {
                                "sin" => q.value.sin(),
                                "cos" => q.value.cos(),
                                _ => q.value.tan(),
                            })
                        }
                        "sqrt" => {
                            if q.length % 2 != 0 || q.angle % 2 != 0 {
                                return Err(CadError::Invalid(
                                    "sqrt requires even unit powers".into(),
                                ));
                            }
                            Quantity {
                                value: q.value.sqrt(),
                                length: q.length / 2,
                                angle: q.angle / 2,
                            }
                        }
                        "abs" => Quantity {
                            value: q.value.abs(),
                            ..q
                        },
                        _ => return Err(CadError::Invalid(format!("Unknown function {n}"))),
                    }
                } else {
                    unit(&n).map(Ok).unwrap_or_else(|| (self.resolve)(&n))?
                }
            }
            Token::Op('-') => {
                let q = self.parse(6)?;
                Quantity {
                    value: -q.value,
                    ..q
                }
            }
            Token::Op('+') => self.parse(6)?,
            Token::Op('(') => {
                let q = self.parse(0)?;
                if !matches!(self.tokens[self.index], Token::Op(')')) {
                    return Err(CadError::Invalid("Expected closing parenthesis".into()));
                }
                self.index += 1;
                q
            }
            _ => return Err(CadError::Invalid("Expected a value".into())),
        };
        while let Token::Op(op) = self.tokens[self.index] {
            let (a, b) = match op {
                '+' | '-' => (1, 2),
                '*' | '/' => (3, 4),
                '^' => (5, 5),
                _ => break,
            };
            if a < min {
                break;
            }
            self.index += 1;
            let r = self.parse(b)?;
            left = match op {
                '+' | '-' => {
                    if left.length != r.length || left.angle != r.angle {
                        return Err(CadError::Invalid("Cannot add incompatible units".into()));
                    }
                    Quantity {
                        value: if op == '+' {
                            left.value + r.value
                        } else {
                            left.value - r.value
                        },
                        ..left
                    }
                }
                '*' => mul(left, r, 1),
                '/' => {
                    if r.value == 0. {
                        return Err(CadError::Invalid("Division by zero".into()));
                    }
                    mul(left, r, -1)
                }
                '^' => {
                    if r.length != 0 || r.angle != 0 || r.value.fract() != 0. || r.value.abs() > 64.
                    {
                        return Err(CadError::Invalid(
                            "Unit powers require integer scalar exponents".into(),
                        ));
                    }
                    Quantity {
                        value: left.value.powf(r.value),
                        length: left.length * r.value as i32,
                        angle: left.angle * r.value as i32,
                    }
                }
                _ => unreachable!(),
            };
        }
        if !left.value.is_finite() {
            return Err(CadError::Invalid("Expression is not finite".into()));
        }
        Ok(left)
    }
}
fn mul(a: Quantity, b: Quantity, sign: i32) -> Quantity {
    Quantity {
        value: if sign == 1 {
            a.value * b.value
        } else {
            a.value / b.value
        },
        length: a.length + sign * b.length,
        angle: a.angle + sign * b.angle,
    }
}
/// Names referenced by an expression, excluding units and function names.
/// Uses the authoritative parser's tokens so graph links follow expression syntax.
pub fn references(expression: &str) -> Result<Vec<String>> {
    let tokens = tokenize(expression)?;
    let mut names = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if let Token::Name(name) = token
            && unit(name).is_none()
            && !matches!(tokens.get(index + 1), Some(Token::Op('(')))
        {
            names.push(name.clone());
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

pub fn evaluate(
    expression: &str,
    mut resolve: impl FnMut(&str) -> Result<Quantity>,
) -> Result<Quantity> {
    let mut p = Parser {
        tokens: tokenize(expression)?,
        index: 0,
        resolve: &mut resolve,
    };
    let q = p.parse(0)?;
    if !matches!(p.tokens[p.index], Token::End) {
        return Err(CadError::Invalid("Unexpected expression suffix".into()));
    }
    Ok(q)
}
pub fn value(doc: &Document, definition: &str, expr: &Expr, dimension: Dimension) -> Result<f64> {
    fn resolve(
        doc: &Document,
        def: &str,
        name: &str,
        stack: &mut BTreeSet<String>,
    ) -> Result<Quantity> {
        let (owner, key) = name.rsplit_once("::").unwrap_or((def, name));
        let owner = if doc.parts.contains_key(owner) || doc.assemblies.contains_key(owner) {
            owner
        } else {
            let names = doc
                .parts
                .values()
                .filter(|p| p.name == owner)
                .map(|p| p.id.as_str())
                .chain(
                    doc.assemblies
                        .values()
                        .filter(|a| a.name == owner)
                        .map(|a| a.id.as_str()),
                )
                .collect::<Vec<_>>();
            if names.len() > 1 {
                return Err(CadError::Invalid(format!(
                    "Ambiguous parameter owner {owner}; use its stable definition ID"
                )));
            }
            names.first().copied().unwrap_or(owner)
        };
        let unique = format!("{owner}::{key}");
        if !stack.insert(unique.clone()) {
            return Err(CadError::Invalid(format!(
                "Parameter dependency cycle {unique}"
            )));
        }
        let params = doc
            .parts
            .get(owner)
            .map(|p| &p.parameters)
            .or_else(|| doc.assemblies.get(owner).map(|a| &a.parameters))
            .ok_or_else(|| CadError::Invalid(format!("Missing parameter owner {owner}")))?;
        let p = params
            .get(key)
            .or_else(|| params.values().find(|p| p.name == key))
            .ok_or_else(|| CadError::Invalid(format!("Unknown parameter {name}")))?;
        let q = evaluate(&p.expression.0, |n| resolve(doc, owner, n, stack))?;
        stack.remove(&unique);
        if !q.matches(p.dimension) {
            return Err(CadError::Invalid(format!("Incorrect units for {}", p.name)));
        }
        Ok(q)
    }
    let q = evaluate(&expr.0, |name| {
        resolve(doc, definition, name, &mut BTreeSet::new())
    })?;
    if q.matches(dimension) {
        Ok(q.value)
    } else if q.length == 0 && q.angle == 0 {
        Ok(if dimension == Dimension::Angle {
            q.value.to_radians()
        } else {
            q.value
        })
    } else {
        Err(CadError::Invalid("Incorrect expression dimension".into()))
    }
}
