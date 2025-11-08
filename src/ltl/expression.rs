use std::convert::TryFrom;
use std::fmt;

use super::parser::{lexer::Lexer, parser};

#[derive(Debug, Eq, PartialEq)]
pub enum LTLExpressionError {
    True,
    False,

    InvalidVariable,

    InvalidOperation,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum LTLExpression {
    True,
    False,
    Literal(String),
    Not(Box<LTLExpression>),
    And(Box<LTLExpression>, Box<LTLExpression>),
    Or(Box<LTLExpression>, Box<LTLExpression>),
    G(Box<LTLExpression>),
    F(Box<LTLExpression>),
    U(Box<LTLExpression>, Box<LTLExpression>),
    R(Box<LTLExpression>, Box<LTLExpression>),
    V(Box<LTLExpression>, Box<LTLExpression>),
}

impl TryFrom<&str> for LTLExpression {
    type Error = &'static str;

    fn try_from(formula: &str) -> Result<Self, Self::Error> {
        let lexer = Lexer::new(formula);
        parser::parse(lexer).map(|span| span.expr).map_err(|e| e.1)
    }
}

impl fmt::Display for LTLExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LTLExpression::True => write!(f, "T"),
            LTLExpression::False => write!(f, "⊥"),
            LTLExpression::Literal(l) => write!(f, "{}", l),
            LTLExpression::Not(e) => write!(f, "¬{}", e),
            LTLExpression::And(e1, e2) => write!(f, "{} ∧ {}", e1, e2),
            LTLExpression::Or(e1, e2) => write!(f, "{} ∨ {}", e1, e2),
            LTLExpression::G(e) => write!(f, "G ({})", e),
            LTLExpression::F(e) => write!(f, "F ({})", e),
            LTLExpression::U(e1, e2) => write!(f, "({} U {})", e1, e2),
            LTLExpression::R(e1, e2) => write!(f, "({} R {})", e1, e2),
            LTLExpression::V(e1, e2) => write!(f, "({} V {})", e1, e2),
        }
    }
}

impl LTLExpression {
    pub fn rewrite(&mut self) {
        *self = rewrite(self.clone())
    }
}

pub fn rewrite(ltle: LTLExpression) -> LTLExpression {
    match ltle {
        LTLExpression::True => LTLExpression::True,
        LTLExpression::False => LTLExpression::False,
        LTLExpression::Literal(l) => LTLExpression::Literal(l),
        LTLExpression::Not(e) => LTLExpression::Not(Box::new(rewrite(*e))),
        LTLExpression::And(e1, e2) => {
            LTLExpression::And(Box::new(rewrite(*e1)), Box::new(rewrite(*e2)))
        }
        LTLExpression::Or(e1, e2) => {
            LTLExpression::Or(Box::new(rewrite(*e1)), Box::new(rewrite(*e2)))
        }
        LTLExpression::G(e) => {
            LTLExpression::R(Box::new(LTLExpression::False), Box::new(rewrite(*e)))
        }
        LTLExpression::F(e) => {
            LTLExpression::U(Box::new(LTLExpression::True), Box::new(rewrite(*e)))
        }
        LTLExpression::U(e1, e2) => {
            LTLExpression::U(Box::new(rewrite(*e1)), Box::new(rewrite(*e2)))
        }
        LTLExpression::R(e1, e2) => {
            LTLExpression::R(Box::new(rewrite(*e1)), Box::new(rewrite(*e2)))
        }
        LTLExpression::V(e1, e2) => LTLExpression::Not(Box::new(LTLExpression::U(
            Box::new(LTLExpression::Not(Box::new(rewrite(*e1)))),
            Box::new(LTLExpression::Not(Box::new(rewrite(*e2)))),
        ))),
    }
}

pub fn put_in_nnf(ltle: LTLExpression) -> LTLExpression {
    match ltle {
        LTLExpression::True => LTLExpression::True,
        LTLExpression::False => LTLExpression::False,
        LTLExpression::Literal(l) => LTLExpression::Literal(l),
        LTLExpression::Not(e) => match *e {
            LTLExpression::True => LTLExpression::False,
            LTLExpression::False => LTLExpression::True,
            LTLExpression::Literal(l) => LTLExpression::Not(Box::new(LTLExpression::Literal(l))),
            LTLExpression::And(e1, e2) => LTLExpression::Or(
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e1))))),
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e2))))),
            ),
            LTLExpression::Or(e1, e2) => LTLExpression::And(
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e1))))),
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e2))))),
            ),
            LTLExpression::G(e) => LTLExpression::F(Box::new(put_in_nnf(LTLExpression::Not(
                Box::new(put_in_nnf(*e)),
            )))),
            LTLExpression::F(e) => LTLExpression::G(Box::new(put_in_nnf(LTLExpression::Not(
                Box::new(put_in_nnf(*e)),
            )))),
            LTLExpression::U(e1, e2) => LTLExpression::V(
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e1))))),
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e2))))),
            ),
            LTLExpression::R(e1, e2) => LTLExpression::U(
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e1))))),
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e2))))),
            ),
            LTLExpression::V(e1, e2) => LTLExpression::U(
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e1))))),
                Box::new(put_in_nnf(LTLExpression::Not(Box::new(put_in_nnf(*e2))))),
            ),
            _ => unimplemented!(),
        },
        LTLExpression::And(e1, e2) => {
            LTLExpression::Or(Box::new(put_in_nnf(*e1)), Box::new(put_in_nnf(*e2)))
        }
        LTLExpression::Or(e1, e2) => {
            LTLExpression::And(Box::new(put_in_nnf(*e1)), Box::new(put_in_nnf(*e2)))
        }
        LTLExpression::G(e) => LTLExpression::G(Box::new(put_in_nnf(*e))),
        LTLExpression::F(e) => LTLExpression::F(Box::new(put_in_nnf(*e))),
        LTLExpression::U(e1, e2) => {
            LTLExpression::U(Box::new(put_in_nnf(*e1)), Box::new(put_in_nnf(*e2)))
        }
        LTLExpression::R(e1, e2) => {
            LTLExpression::R(Box::new(put_in_nnf(*e1)), Box::new(put_in_nnf(*e2)))
        }
        LTLExpression::V(e1, e2) => {
            LTLExpression::V(Box::new(put_in_nnf(*e1)), Box::new(put_in_nnf(*e2)))
        }
    }
}

#[cfg(test)]
mod test_ltl_expression {
    use super::*;

    #[test]
    fn test_put_in_nnf() {
        let expr = LTLExpression::Not(Box::new(LTLExpression::U(
            Box::new(LTLExpression::Literal("p".to_owned())),
            Box::new(LTLExpression::Literal("q".to_owned())),
        )));

        let expected_nnf = LTLExpression::V(
            Box::new(LTLExpression::Not(Box::new(LTLExpression::Literal(
                "p".to_owned(),
            )))),
            Box::new(LTLExpression::Not(Box::new(LTLExpression::Literal(
                "q".to_owned(),
            )))),
        );

        assert_eq!(expected_nnf, put_in_nnf(expr));
    }
}
