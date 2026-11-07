//! A small arithmetic expression evaluator for amount fields.
//!
//! Supports `+ - * /` (also `×` and `÷`), parentheses, unary signs, decimal
//! numbers with optional thousands separators (in the chosen number style), and the `k` (×1 000)
//! and `m` (×1 000 000) suffixes on numbers and parenthesised groups. All
//! arithmetic is exact [`Decimal`] arithmetic with overflow checks.
//!
//! This module has no GPUI dependency.

use std::fmt;

use rust_decimal::Decimal;

use crate::money::NumberStyle;

/// Why an expression could not be evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprError {
    /// The input has no tokens.
    Empty,
    /// A character that is not part of the grammar, at this byte offset.
    UnexpectedChar(usize),
    /// The expression ended where an operand was expected.
    UnexpectedEnd,
    /// Tokens remain after a complete expression, at this byte offset.
    TrailingInput(usize),
    /// A `(` without its `)`.
    UnclosedParen,
    /// A number literal could not be read.
    BadNumber,
    DivisionByZero,
    Overflow,
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty expression"),
            Self::UnexpectedChar(at) => write!(f, "unexpected character at {at}"),
            Self::UnexpectedEnd => write!(f, "incomplete expression"),
            Self::TrailingInput(at) => write!(f, "unexpected input at {at}"),
            Self::UnclosedParen => write!(f, "missing closing parenthesis"),
            Self::BadNumber => write!(f, "invalid number"),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::Overflow => write!(f, "number too large"),
        }
    }
}

impl std::error::Error for ExprError {}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(Decimal),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    /// A magnitude suffix: `k` or `m`.
    Suffix(Decimal),
}

/// Stands for a `.` in `1.234,56` style until the literal is read whole.
const MAYBE_POINT: char = '\'';

/// In `1.234,56` style, whether the `.`s (marked [`MAYBE_POINT`]) of a
/// literal are grouping or a decimal point. With a `,` in the literal they
/// group; without one they group only when the literal is shaped like
/// grouped thousands (`1.234`, `12.345.678`), so `12.5` and `0.250` keep
/// working as decimals.
fn resolve_dots(literal: &str) -> String {
    if !literal.contains(MAYBE_POINT) {
        return literal.to_string();
    }
    let grouped = match literal.find('.') {
        // Grouping only before the decimal comma (`1,5.5` is no number).
        Some(point) => !literal[point..].contains(MAYBE_POINT),
        None => {
            let mut parts = literal.split(MAYBE_POINT);
            let lead = parts.next().unwrap_or_default();
            (1..=3).contains(&lead.len())
                && !lead.starts_with('0')
                && parts.all(|part| part.len() == 3)
        }
    };
    if grouped {
        literal.replace(MAYBE_POINT, "")
    } else {
        literal.replace(MAYBE_POINT, ".")
    }
}

fn tokenize(input: &str, style: NumberStyle) -> Result<Vec<(usize, Token)>, ExprError> {
    // Grouping separators are accepted and ignored; the decimal separator
    // reads as the point. In the comma-decimal styles `,` is only ever the
    // point (never grouping), and `.` is read as well: in `1 234,56` it is a
    // point, and in `1.234,56` it groups or is a point by the literal's
    // shape (see `resolve_dots`).
    let dots_group = style.group == '.';
    let is_point = |c: char| c == style.decimal || (c == '.' && !dots_group);
    let is_group = |c: char| {
        !is_point(c) && (c == style.group || matches!(c, ',' | '_' | '\u{202f}' | '\u{a0}'))
    };
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some(&(at, c)) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            c if c.is_ascii_digit() || is_point(c) || c == '.' => {
                let mut literal = String::new();
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_ascii_digit() {
                        literal.push(c);
                    } else if is_point(c) {
                        literal.push('.');
                    } else if dots_group && c == '.' {
                        literal.push(MAYBE_POINT);
                    } else if !is_group(c) {
                        break;
                    }
                    chars.next();
                }
                let literal = resolve_dots(&literal);
                if literal == "." || literal.is_empty() || literal.matches('.').count() > 1 {
                    return Err(ExprError::BadNumber);
                }
                let value = literal
                    .parse::<Decimal>()
                    .map_err(|_| ExprError::BadNumber)?;
                tokens.push((at, Token::Num(value)));
            }
            '+' => {
                chars.next();
                tokens.push((at, Token::Plus));
            }
            '-' | '−' => {
                chars.next();
                tokens.push((at, Token::Minus));
            }
            '*' | '×' | 'x' | 'X' => {
                chars.next();
                tokens.push((at, Token::Star));
            }
            '/' | '÷' => {
                chars.next();
                tokens.push((at, Token::Slash));
            }
            '(' => {
                chars.next();
                tokens.push((at, Token::LParen));
            }
            ')' => {
                chars.next();
                tokens.push((at, Token::RParen));
            }
            'k' | 'K' => {
                chars.next();
                tokens.push((at, Token::Suffix(Decimal::from(1_000))));
            }
            'm' | 'M' => {
                chars.next();
                tokens.push((at, Token::Suffix(Decimal::from(1_000_000))));
            }
            _ => return Err(ExprError::UnexpectedChar(at)),
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<(usize, Token)>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(_, t)| t)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).map(|(_, t)| t.clone());
        self.pos += 1;
        token
    }

    /// expr := term (('+' | '-') term)*
    fn expr(&mut self) -> Result<Decimal, ExprError> {
        let mut value = self.term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.next();
                    let rhs = self.term()?;
                    value = value.checked_add(rhs).ok_or(ExprError::Overflow)?;
                }
                Some(Token::Minus) => {
                    self.next();
                    let rhs = self.term()?;
                    value = value.checked_sub(rhs).ok_or(ExprError::Overflow)?;
                }
                _ => return Ok(value),
            }
        }
    }

    /// term := unary (('*' | '/') unary)*
    fn term(&mut self) -> Result<Decimal, ExprError> {
        let mut value = self.unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.next();
                    let rhs = self.unary()?;
                    value = value.checked_mul(rhs).ok_or(ExprError::Overflow)?;
                }
                Some(Token::Slash) => {
                    self.next();
                    let rhs = self.unary()?;
                    if rhs.is_zero() {
                        return Err(ExprError::DivisionByZero);
                    }
                    value = value.checked_div(rhs).ok_or(ExprError::Overflow)?;
                }
                _ => return Ok(value),
            }
        }
    }

    /// unary := ('+' | '-') unary | primary
    fn unary(&mut self) -> Result<Decimal, ExprError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.next();
                Ok(-self.unary()?)
            }
            Some(Token::Plus) => {
                self.next();
                self.unary()
            }
            _ => self.primary(),
        }
    }

    /// primary := (number | '(' expr ')') suffix*
    fn primary(&mut self) -> Result<Decimal, ExprError> {
        let mut value = match self.next() {
            Some(Token::Num(n)) => n,
            Some(Token::LParen) => {
                let inner = self.expr()?;
                match self.next() {
                    Some(Token::RParen) => inner,
                    None => return Err(ExprError::UnclosedParen),
                    Some(_) => return Err(ExprError::TrailingInput(self.offset(self.pos - 1))),
                }
            }
            None => return Err(ExprError::UnexpectedEnd),
            Some(_) => return Err(ExprError::UnexpectedChar(self.offset(self.pos - 1))),
        };
        while let Some(Token::Suffix(mul)) = self.peek() {
            let mul = *mul;
            self.next();
            value = value.checked_mul(mul).ok_or(ExprError::Overflow)?;
        }
        Ok(value)
    }

    fn offset(&self, pos: usize) -> usize {
        self.tokens.get(pos).map(|(at, _)| *at).unwrap_or(0)
    }
}

/// Evaluates an amount expression such as `120*3+15`, `1.5k` or `(80+40)/3`.
#[cfg(test)]
pub(crate) fn eval(input: &str) -> Result<Decimal, ExprError> {
    eval_in(input, NumberStyle::COMMA_DOT)
}

/// [`eval`] with numbers written in `style` (`1.234,5*2` in `1.234,56`).
pub fn eval_in(input: &str, style: NumberStyle) -> Result<Decimal, ExprError> {
    let tokens = tokenize(input, style)?;
    if tokens.is_empty() {
        return Err(ExprError::Empty);
    }
    let mut parser = Parser { tokens, pos: 0 };
    let value = parser.expr()?;
    if parser.pos < parser.tokens.len() {
        return Err(ExprError::TrailingInput(parser.offset(parser.pos)));
    }
    Ok(value.normalize())
}

/// Whether the input is a single plain number (no operators or suffixes), so a
/// "= result" hint would only repeat it.
#[cfg(test)]
pub(crate) fn is_plain_number(input: &str) -> bool {
    is_plain_number_in(input, NumberStyle::COMMA_DOT)
}

/// [`is_plain_number`] with numbers written in `style`.
pub fn is_plain_number_in(input: &str, style: NumberStyle) -> bool {
    matches!(tokenize(input, style).as_deref(), Ok([(_, Token::Num(_))]))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn numbers() {
        assert_eq!(eval("100"), Ok(d("100")));
        assert_eq!(eval("  89.56 "), Ok(d("89.56")));
        assert_eq!(eval(".5"), Ok(d("0.5")));
        assert_eq!(eval("15,832"), Ok(d("15832")));
        assert_eq!(eval("1_000.25"), Ok(d("1000.25")));
    }

    #[test]
    fn spec_examples() {
        assert_eq!(eval("120*3+15"), Ok(d("375")));
        assert_eq!(eval("1.5k"), Ok(d("1500")));
        assert_eq!(eval("(80+40)/3"), Ok(d("40")));
        assert_eq!(eval("1.5k*3"), Ok(d("4500")));
        assert_eq!(eval("120*3"), Ok(d("360")));
    }

    #[test]
    fn precedence_and_unary() {
        assert_eq!(eval("2+3*4"), Ok(d("14")));
        assert_eq!(eval("(2+3)*4"), Ok(d("20")));
        assert_eq!(eval("-5+2"), Ok(d("-3")));
        assert_eq!(eval("2*-3"), Ok(d("-6")));
        assert_eq!(eval("--4"), Ok(d("4")));
        assert_eq!(eval("10-2-3"), Ok(d("5")));
        assert_eq!(eval("100/4/5"), Ok(d("5")));
        assert_eq!(eval("2 × 3 ÷ 4"), Ok(d("1.5")));
    }

    #[test]
    fn suffixes() {
        assert_eq!(eval("2m"), Ok(d("2000000")));
        assert_eq!(eval("2K"), Ok(d("2000")));
        assert_eq!(eval("(1+1)k"), Ok(d("2000")));
        assert_eq!(eval("1.25m/5"), Ok(d("250000")));
    }

    #[test]
    fn exact_decimal() {
        assert_eq!(eval("0.1+0.2"), Ok(d("0.3")));
        assert_eq!(eval("1/3*3").map(|v| v.round_dp(10)), Ok(d("1")));
    }

    #[test]
    fn errors() {
        assert_eq!(eval(""), Err(ExprError::Empty));
        assert_eq!(eval("   "), Err(ExprError::Empty));
        assert_eq!(eval("3**"), Err(ExprError::UnexpectedChar(2)));
        assert_eq!(eval("3*"), Err(ExprError::UnexpectedEnd));
        assert_eq!(eval("(1+2"), Err(ExprError::UnclosedParen));
        assert_eq!(eval("1+2)"), Err(ExprError::TrailingInput(3)));
        assert_eq!(eval("1/0"), Err(ExprError::DivisionByZero));
        assert_eq!(eval("1/(2-2)"), Err(ExprError::DivisionByZero));
        assert_eq!(eval("1.2.3"), Err(ExprError::BadNumber));
        assert_eq!(eval("abc"), Err(ExprError::UnexpectedChar(0)));
        assert_eq!(eval("k"), Err(ExprError::UnexpectedChar(0)));
        assert_eq!(eval("2 3"), Err(ExprError::TrailingInput(2)));
        assert_eq!(
            eval("99999999999999999999m*99999999999m"),
            Err(ExprError::Overflow)
        );
    }

    #[test]
    fn plain_number() {
        assert!(is_plain_number("100"));
        assert!(is_plain_number("1,234.5"));
        assert!(!is_plain_number("1k"));
        assert!(!is_plain_number("1+1"));
        assert!(!is_plain_number(""));
        assert!(!is_plain_number("-3"));
    }

    /// With a comma decimal, `,` is always the point and never groups, and
    /// `.` still works as a point unless it is plainly grouping thousands.
    #[test]
    fn comma_decimal_expressions() {
        for style in [NumberStyle::DOT_COMMA, NumberStyle::SPACE_COMMA] {
            assert_eq!(eval_in("12,5*2", style), Ok(d("25")), "{style:?}");
            assert_eq!(eval_in(",5", style), Ok(d("0.5")));
            assert_eq!(eval_in("12.5*2", style), Ok(d("25")));
            assert_eq!(eval_in(".5+,5", style), Ok(d("1")));
            assert_eq!(eval_in("0,1+0,2", style), Ok(d("0.3")));
            assert_eq!(eval_in("1,5k*3", style), Ok(d("4500")));
            assert_eq!(eval_in("(1,5+2,5)/2", style), Ok(d("2")));
            assert_eq!(eval_in("100/3,2", style), Ok(d("31.25")));
            // Never thousands: `1,234` is a decimal, `1,2,3` is no number.
            assert_eq!(eval_in("1,234", style), Ok(d("1.234")));
            assert_eq!(eval_in("1,2,3", style), Err(ExprError::BadNumber));
            assert_eq!(eval_in("1,5.5", style), Err(ExprError::BadNumber));
            assert_eq!(eval_in(",", style), Err(ExprError::BadNumber));
            assert!(is_plain_number_in("12,5", style));
            assert!(!is_plain_number_in("12,5*2", style));
        }
        let dot = NumberStyle::DOT_COMMA;
        // `.` groups next to a decimal comma, or in a grouped shape.
        assert_eq!(eval_in("1.234,56", dot), Ok(d("1234.56")));
        assert_eq!(eval_in("1.234", dot), Ok(d("1234")));
        assert_eq!(eval_in("12.345.678", dot), Ok(d("12345678")));
        assert_eq!(eval_in("1.234*2", dot), Ok(d("2468")));
        // Otherwise it is a point.
        assert_eq!(eval_in("0.250", dot), Ok(d("0.25")));
        assert_eq!(eval_in("1.5k", dot), Ok(d("1500")));
        assert_eq!(eval_in("1234.5", dot), Ok(d("1234.5")));
        assert_eq!(eval_in("1.2.3", dot), Err(ExprError::BadNumber));
        assert_eq!(eval_in("1.234.5", dot), Err(ExprError::BadNumber));
        // `1 234,56` groups with a narrow or plain no-break space, and a `.`
        // is always a point there.
        let space = NumberStyle::SPACE_COMMA;
        assert_eq!(eval_in("1\u{202f}234,56", space), Ok(d("1234.56")));
        assert_eq!(eval_in("1\u{a0}234.5", space), Ok(d("1234.5")));
        assert_eq!(eval_in("2 3", space), Err(ExprError::TrailingInput(2)));
        assert_eq!(eval_in("1.234", space), Ok(d("1.234")));
        // The default reads `,` as grouping.
        assert_eq!(eval_in("1,234.5", NumberStyle::COMMA_DOT), Ok(d("1234.5")));
    }
}
