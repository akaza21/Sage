//! A question's answer limits and the number grammar answers are read with.
//! The question card mirrors both (`UserInputField.tsx`), so Submit stays
//! disabled for an answer admission would refuse.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Number;

use super::{UserInputError, UserInputKind};

/// Limits a form schema puts on a question's answer; an absent one does not
/// apply. Lengths count characters (Unicode scalar values), not bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserInputLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    /// Inclusive bounds of a number or integer answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u32>,
}

impl UserInputLimits {
    pub(crate) fn is_unset(&self) -> bool {
        *self == Self::default()
    }

    pub fn is_ordered(&self) -> bool {
        let numbers = match (&self.minimum, &self.maximum) {
            (Some(minimum), Some(maximum)) => compare(minimum, maximum).is_le(),
            _ => true,
        };
        numbers
            && self
                .min_length
                .zip(self.max_length)
                .is_none_or(|(a, b)| a <= b)
            && self
                .min_items
                .zip(self.max_items)
                .is_none_or(|(a, b)| a <= b)
    }

    pub(super) fn check_text(&self, kind: UserInputKind, text: &str) -> Result<(), UserInputError> {
        if !within(text.chars().count(), self.min_length, self.max_length) {
            return Err(UserInputError::TextLength);
        }
        let value = match kind {
            UserInputKind::Integer => parse_integer(text.trim()).map(Number::from),
            UserInputKind::Number => parse_number(text.trim()).and_then(Number::from_f64),
            _ => return Ok(()),
        }
        .ok_or(UserInputError::NotANumber)?;
        let below = self
            .minimum
            .as_ref()
            .is_some_and(|minimum| compare(&value, minimum).is_lt());
        let above = self
            .maximum
            .as_ref()
            .is_some_and(|maximum| compare(&value, maximum).is_gt());
        if below || above {
            return Err(UserInputError::OutOfRange);
        }
        Ok(())
    }

    pub(super) fn check_items(&self, chosen: usize) -> Result<(), UserInputError> {
        if within(chosen, self.min_items, self.max_items) {
            Ok(())
        } else {
            Err(UserInputError::ItemCount)
        }
    }
}

fn within(count: usize, min: Option<u32>, max: Option<u32>) -> bool {
    min.is_none_or(|min| count >= min as usize) && max.is_none_or(|max| count <= max as usize)
}

/// Integers compare exactly; anything else compares as `f64`.
pub(crate) fn compare(left: &Number, right: &Number) -> Ordering {
    match (left.as_i64(), right.as_i64()) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left
            .as_f64()
            .zip(right.as_f64())
            .and_then(|(left, right)| left.partial_cmp(&right))
            .unwrap_or(Ordering::Equal),
    }
}

fn digits(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii_digit())
}

fn unsigned(text: &str) -> &str {
    text.strip_prefix(['+', '-']).unwrap_or(text)
}

/// An integer answer: `^[+-]?\d+$` within the `i64` range.
pub(crate) fn parse_integer(text: &str) -> Option<i64> {
    let magnitude = unsigned(text);
    (!magnitude.is_empty() && digits(magnitude))
        .then(|| text.parse().ok())
        .flatten()
}

/// A number answer: a plain finite decimal,
/// `^[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?$` (no hex, `Infinity` or `NaN`).
pub(crate) fn parse_number(text: &str) -> Option<f64> {
    let rest = unsigned(text);
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], Some(unsigned(&rest[at + 1..]))),
        None => (rest, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let has_digits = !whole.is_empty() || !fraction.is_empty();
    let grammar = has_digits
        && digits(whole)
        && digits(fraction)
        && exponent.is_none_or(|exponent| !exponent.is_empty() && digits(exponent));
    grammar
        .then(|| text.parse::<f64>().ok())
        .flatten()
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_one_plain_decimal_grammar() {
        let cases = [
            ("1.0", None, Some(1.0)),
            ("1e3", None, Some(1000.0)),
            ("0x10", None, None),
            (" 1", None, None),
            ("", None, None),
            ("-0", Some(0), Some(-0.0)),
            ("+5", Some(5), Some(5.0)),
            (".5", None, Some(0.5)),
            ("1.", None, Some(1.0)),
            (".", None, None),
            ("1e", None, None),
            ("inf", None, None),
            ("NaN", None, None),
            ("1e999", None, None),
            (
                "9223372036854775807",
                Some(i64::MAX),
                Some(9.223_372_036_854_776e18),
            ),
            ("9223372036854775808", None, Some(9.223_372_036_854_776e18)),
        ];
        for (text, integer, number) in cases {
            assert_eq!(parse_integer(text), integer, "{text:?}");
            assert_eq!(parse_number(text), number, "{text:?}");
        }
    }
}
