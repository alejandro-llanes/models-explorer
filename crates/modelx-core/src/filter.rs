//! CLI-style filter expression parser and evaluator for [`Model`]s.
//!
//! ## Syntax
//!
//! ```text
//! field OP value
//! ```
//!
//! **Symbol operators** (embedded, no whitespace required):
//! `<=`, `>=`, `!=`, `!~`, `<`, `>`, `=`, `~`
//!
//! **Word operators** (whitespace-separated, only when no symbol found):
//! `lt`, `lte`, `eq`, `ne`, `gte`, `gt`, `contains`, `ncontains`
//!
//! ## Examples
//! ```text
//! input_cost<=3
//! context_limit>=200000
//! name~claude opus
//! reasoning=true
//! release_date>=2025-01-01
//! provider_name~anthropic
//! input_modalities contains image
//! ```

use regex::Regex;
use thiserror::Error;

use crate::field::{Field, FieldKind, FieldValue};
use crate::model::Model;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Comparison operators supported by the filter engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `<` / `lt`
    Lt,
    /// `<=` / `lte`
    Le,
    /// `=` / `eq`
    Eq,
    /// `!=` / `ne`
    Ne,
    /// `>=` / `gte`
    Ge,
    /// `>` / `gt`
    Gt,
    /// `~` / `contains`
    Contains,
    /// `!~` / `ncontains`
    NotContains,
}

impl Op {
    fn as_str(self) -> &'static str {
        match self {
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Eq => "=",
            Op::Ne => "!=",
            Op::Ge => ">=",
            Op::Gt => ">",
            Op::Contains => "~",
            Op::NotContains => "!~",
        }
    }
}

/// Errors that can occur while parsing a filter expression.
#[derive(Debug, Error, PartialEq)]
pub enum FilterError {
    /// The expression was empty (or only whitespace).
    #[error("filter expression is empty")]
    Empty,

    /// No operator was found in the expression.
    #[error("no operator found in filter expression: {0:?}")]
    NoOperator(String),

    /// The field name is not recognised.
    #[error("unknown field: {0:?}")]
    UnknownField(String),

    /// The operator is not valid for the field's kind (e.g. `<` on a Bool field).
    #[error("operator {op:?} is not valid for field {field:?}")]
    NotComparable { field: String, op: String },

    /// The value could not be parsed as a number.
    #[error("expected a number, got: {0:?}")]
    BadNumber(String),

    /// The regex pattern is invalid.
    #[error("invalid regular expression: {0:?}")]
    BadRegex(String),
}

// ---------------------------------------------------------------------------
// Stored target value (pre-parsed at `parse` time)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Target {
    /// Pre-parsed `f64` for numeric comparisons.
    Number(f64),
    /// Case-folded plain string for text / bool / list comparisons.
    Text(String),
    /// Compiled regex (case-insensitive) for `~` / `!~` with `regex=true`.
    Pattern(Regex),
}

// ---------------------------------------------------------------------------
// Predicate
// ---------------------------------------------------------------------------

/// A parsed, ready-to-evaluate filter predicate.
///
/// Does not implement `PartialEq`/`Eq` because [`Regex`] is not `Eq`.
#[derive(Clone, Debug)]
pub struct Predicate {
    field: Field,
    op: Op,
    target: Target,
}

impl Predicate {
    /// Parse a `"field OP value"` expression.
    ///
    /// When `regex` is `true`, the target of `~` / `!~` operators is compiled
    /// as a case-insensitive regular expression instead of a plain substring.
    pub fn parse(expr: &str, regex: bool) -> Result<Predicate, FilterError> {
        let expr = expr.trim();
        if expr.is_empty() {
            return Err(FilterError::Empty);
        }

        // --- locate operator -------------------------------------------------
        let (field_str, op, value_str) = locate_operator(expr)?;

        // --- resolve field ---------------------------------------------------
        let field = Field::from_key(field_str.trim())
            .ok_or_else(|| FilterError::UnknownField(field_str.trim().to_string()))?;

        let value_str = value_str.trim();

        // --- validate operator vs field kind ---------------------------------
        validate_op(&field, op)?;

        // --- pre-parse the target value --------------------------------------
        let target = build_target(&field, op, value_str, regex)?;

        Ok(Predicate { field, op, target })
    }

    /// The field this predicate tests.
    pub fn field(&self) -> Field {
        self.field
    }

    /// Evaluate this predicate against `m`, returning `true` if it matches.
    pub fn matches(&self, m: &Model) -> bool {
        let value = self.field.value(m);
        match self.field.kind() {
            FieldKind::Number => match_number(&value, self.op, &self.target),
            FieldKind::Text => match_text(&value, self.op, &self.target),
            FieldKind::Bool => match_bool(&value, self.op, &self.target),
            FieldKind::List => match_list(&value, self.op, &self.target),
        }
    }
}

// ---------------------------------------------------------------------------
// parse_filters / matches_all
// ---------------------------------------------------------------------------

/// Parse a slice of filter expressions into a `Vec<Predicate>`.
///
/// Returns the first error encountered.
pub fn parse_filters(exprs: &[String], regex: bool) -> Result<Vec<Predicate>, FilterError> {
    exprs.iter().map(|e| Predicate::parse(e, regex)).collect()
}

/// Return `true` if `m` satisfies **all** predicates (AND semantics).
pub fn matches_all(m: &Model, preds: &[Predicate]) -> bool {
    preds.iter().all(|p| p.matches(m))
}

// ---------------------------------------------------------------------------
// Operator location
// ---------------------------------------------------------------------------

/// Extract `(field_str, op, value_str)` from a raw expression.
fn locate_operator(expr: &str) -> Result<(&str, Op, &str), FilterError> {
    // Try symbol operators first.  Scan left-to-right, 2-char before 1-char.
    let two_char_ops: &[(&str, Op)] = &[
        ("<=", Op::Le),
        (">=", Op::Ge),
        ("!=", Op::Ne),
        ("!~", Op::NotContains),
    ];
    let one_char_ops: &[(&str, Op)] = &[
        ("<", Op::Lt),
        (">", Op::Gt),
        ("=", Op::Eq),
        ("~", Op::Contains),
    ];

    // Build a combined ordered scan: try 2-char matches at each position first.
    let len = expr.len();

    for i in 0..len {
        // Try 2-char ops at position i.
        if i + 2 <= len {
            let slice2 = &expr[i..i + 2];
            for &(sym, op) in two_char_ops {
                if slice2 == sym {
                    return Ok((&expr[..i], op, &expr[i + 2..]));
                }
            }
        }
        // Try 1-char ops at position i.
        if i < len {
            let slice1 = &expr[i..i + 1];
            for &(sym, op) in one_char_ops {
                // Don't match '!' followed by another char we'd have handled as 2-char.
                // (We already checked 2-char above, so a lone '!' here means no 2-char
                //  matched — the '!' at this position is standalone, which is not a
                //  recognised symbol, so just skip it.)
                if slice1 == sym {
                    // Make sure we're not splitting a multi-byte char.
                    if expr.is_char_boundary(i) && expr.is_char_boundary(i + sym.len()) {
                        return Ok((&expr[..i], op, &expr[i + sym.len()..]));
                    }
                }
            }
        }
    }

    // No symbol operator found — try word operators.
    let tokens: Vec<&str> = expr.split_whitespace().collect();
    if tokens.len() >= 3 {
        let word_op = match tokens[1].to_lowercase().as_str() {
            "lt" => Some(Op::Lt),
            "lte" => Some(Op::Le),
            "eq" => Some(Op::Eq),
            "ne" => Some(Op::Ne),
            "gte" => Some(Op::Ge),
            "gt" => Some(Op::Gt),
            "contains" => Some(Op::Contains),
            "ncontains" => Some(Op::NotContains),
            _ => None,
        };
        if let Some(op) = word_op {
            // Return slices from the original string.
            // For the value we find the position of the third token in the
            // original string so we can return a proper slice.
            let field_end = tokens[0].len();
            let op_start = expr.find(tokens[1]).unwrap_or(field_end);
            let op_end = op_start + tokens[1].len();
            // Find value start as first non-whitespace after op token.
            let value_start = expr[op_end..]
                .find(|c: char| !c.is_whitespace())
                .map(|p| op_end + p);
            if let Some(vs) = value_start {
                return Ok((&expr[..field_end], op, &expr[vs..]));
            }
        }
    }

    Err(FilterError::NoOperator(expr.to_string()))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_op(field: &Field, op: Op) -> Result<(), FilterError> {
    let ordering = matches!(op, Op::Lt | Op::Le | Op::Ge | Op::Gt);
    match field.kind() {
        FieldKind::Bool | FieldKind::List if ordering => {
            return Err(FilterError::NotComparable {
                field: field.key().to_string(),
                op: op.as_str().to_string(),
            });
        }
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Target construction
// ---------------------------------------------------------------------------

fn build_target(
    field: &Field,
    op: Op,
    value_str: &str,
    use_regex: bool,
) -> Result<Target, FilterError> {
    match field.kind() {
        FieldKind::Number => {
            let is_substring_op = matches!(op, Op::Contains | Op::NotContains);
            if is_substring_op {
                // Contains/NotContains on a Number field: match against display string.
                Ok(build_text_target(value_str, op, use_regex)?)
            } else {
                let n: f64 = value_str
                    .parse()
                    .map_err(|_| FilterError::BadNumber(value_str.to_string()))?;
                Ok(Target::Number(n))
            }
        }
        FieldKind::Text => build_text_target(value_str, op, use_regex),
        FieldKind::Bool => {
            // Parse value as a bool.  Stored as case-folded text for comparison.
            let _ = parse_bool(value_str)?; // validate
            Ok(Target::Text(value_str.to_lowercase()))
        }
        FieldKind::List => build_text_target(value_str, op, use_regex),
    }
}

/// Build a `Target::Text` or `Target::Pattern` for string/substring/regex ops.
fn build_text_target(value_str: &str, op: Op, use_regex: bool) -> Result<Target, FilterError> {
    let is_regex_op = matches!(op, Op::Contains | Op::NotContains);
    if use_regex && is_regex_op {
        // Compile the user-supplied pattern as a case-insensitive regex.
        let re = Regex::new(&format!("(?i){value_str}"))
            .map_err(|e| FilterError::BadRegex(e.to_string()))?;
        Ok(Target::Pattern(re))
    } else {
        Ok(Target::Text(value_str.to_lowercase()))
    }
}

// ---------------------------------------------------------------------------
// Matching helpers
// ---------------------------------------------------------------------------

fn match_number(value: &FieldValue, op: Op, target: &Target) -> bool {
    match op {
        Op::Contains | Op::NotContains => {
            // Substring match on the display string.
            let display = value.display().to_lowercase();
            match target {
                Target::Text(t) => {
                    let hit = display.contains(t.as_str());
                    if matches!(op, Op::NotContains) {
                        !hit
                    } else {
                        hit
                    }
                }
                Target::Pattern(re) => {
                    let hit = re.is_match(&display);
                    if matches!(op, Op::NotContains) {
                        !hit
                    } else {
                        hit
                    }
                }
                Target::Number(_) => false,
            }
        }
        _ => {
            let model_val = value.as_f64();
            let Target::Number(target_n) = target else {
                return false;
            };
            match op {
                Op::Eq => model_val.is_some_and(|v| (v - target_n).abs() < f64::EPSILON),
                Op::Ne => model_val.is_none_or(|v| (v - target_n).abs() >= f64::EPSILON),
                Op::Lt => model_val.is_some_and(|v| v < *target_n),
                Op::Le => model_val.is_some_and(|v| v <= *target_n),
                Op::Ge => model_val.is_some_and(|v| v >= *target_n),
                Op::Gt => model_val.is_some_and(|v| v > *target_n),
                Op::Contains | Op::NotContains => unreachable!(),
            }
        }
    }
}

fn match_text(value: &FieldValue, op: Op, target: &Target) -> bool {
    let display = value.display().to_lowercase();
    match op {
        Op::Eq => match target {
            Target::Text(t) => display == *t,
            _ => false,
        },
        Op::Ne => match target {
            Target::Text(t) => display != *t,
            _ => false,
        },
        Op::Lt => match target {
            Target::Text(t) => display < *t,
            _ => false,
        },
        Op::Le => match target {
            Target::Text(t) => display <= *t,
            _ => false,
        },
        Op::Ge => match target {
            Target::Text(t) => display >= *t,
            _ => false,
        },
        Op::Gt => match target {
            Target::Text(t) => display > *t,
            _ => false,
        },
        Op::Contains => match target {
            Target::Text(t) => display.contains(t.as_str()),
            Target::Pattern(re) => re.is_match(&display),
            Target::Number(_) => false,
        },
        Op::NotContains => match target {
            Target::Text(t) => !display.contains(t.as_str()),
            Target::Pattern(re) => !re.is_match(&display),
            Target::Number(_) => false,
        },
    }
}

fn match_bool(value: &FieldValue, op: Op, target: &Target) -> bool {
    let model_bool = match value {
        FieldValue::Bool(Some(b)) => *b,
        _ => false, // None treated as false
    };
    let Target::Text(t) = target else {
        return false;
    };
    // We already validated at parse time that the value is parseable as bool.
    let target_bool = parse_bool(t).unwrap_or(false);
    match op {
        Op::Eq => model_bool == target_bool,
        Op::Ne => model_bool != target_bool,
        // Ordering ops are rejected at parse time.
        _ => false,
    }
}

fn match_list(value: &FieldValue, op: Op, target: &Target) -> bool {
    let items = match value {
        FieldValue::List(v) => v,
        _ => return false,
    };
    match op {
        Op::Eq => items.iter().any(|item| match target {
            Target::Text(t) => item.to_lowercase() == *t,
            _ => false,
        }),
        Op::Ne => !items.iter().any(|item| match target {
            Target::Text(t) => item.to_lowercase() == *t,
            _ => false,
        }),
        Op::Contains => items.iter().any(|item| {
            let lower = item.to_lowercase();
            match target {
                Target::Text(t) => lower.contains(t.as_str()),
                Target::Pattern(re) => re.is_match(&lower),
                Target::Number(_) => false,
            }
        }),
        Op::NotContains => !items.iter().any(|item| {
            let lower = item.to_lowercase();
            match target {
                Target::Text(t) => lower.contains(t.as_str()),
                Target::Pattern(re) => re.is_match(&lower),
                Target::Number(_) => false,
            }
        }),
        // Ordering ops are rejected at parse time.
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Parse a bool from a string, accepting `true/false/yes/no/1/0` (case-insensitive).
fn parse_bool(s: &str) -> Result<bool, FilterError> {
    match s.to_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        _ => Err(FilterError::BadNumber(s.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::sample_catalog;

    fn all_models(catalog: &crate::model::Catalog) -> Vec<&crate::model::Model> {
        catalog.all_models().collect()
    }

    // --- FieldKind assertions ---

    #[test]
    fn field_kind_number() {
        assert_eq!(Field::InputCost.kind(), FieldKind::Number);
        assert_eq!(Field::ContextLimit.kind(), FieldKind::Number);
        assert_eq!(Field::OutputLimit.kind(), FieldKind::Number);
    }

    #[test]
    fn field_kind_bool() {
        assert_eq!(Field::Reasoning.kind(), FieldKind::Bool);
        assert_eq!(Field::OpenWeights.kind(), FieldKind::Bool);
        assert_eq!(Field::ToolCall.kind(), FieldKind::Bool);
    }

    #[test]
    fn field_kind_list() {
        assert_eq!(Field::InputModalities.kind(), FieldKind::List);
        assert_eq!(Field::OutputModalities.kind(), FieldKind::List);
        assert_eq!(Field::ReasoningEfforts.kind(), FieldKind::List);
    }

    #[test]
    fn field_kind_text() {
        assert_eq!(Field::Name.kind(), FieldKind::Text);
        assert_eq!(Field::ProviderName.kind(), FieldKind::Text);
        assert_eq!(Field::ReleaseDate.kind(), FieldKind::Text);
        assert_eq!(Field::Knowledge.kind(), FieldKind::Text);
    }

    // --- Symbol operator parsing ---

    #[test]
    fn parse_symbol_le() {
        let p = Predicate::parse("input_cost<=3", false).unwrap();
        assert_eq!(p.field(), Field::InputCost);
        assert_eq!(p.op, Op::Le);
    }

    #[test]
    fn parse_symbol_ge() {
        let p = Predicate::parse("context_limit>=200000", false).unwrap();
        assert_eq!(p.field(), Field::ContextLimit);
        assert_eq!(p.op, Op::Ge);
    }

    #[test]
    fn parse_symbol_tilde() {
        let p = Predicate::parse("name~opus", false).unwrap();
        assert_eq!(p.field(), Field::Name);
        assert_eq!(p.op, Op::Contains);
    }

    #[test]
    fn parse_symbol_eq() {
        let p = Predicate::parse("reasoning=true", false).unwrap();
        assert_eq!(p.field(), Field::Reasoning);
        assert_eq!(p.op, Op::Eq);
    }

    #[test]
    fn parse_symbol_ne() {
        let p = Predicate::parse("reasoning!=true", false).unwrap();
        assert_eq!(p.field(), Field::Reasoning);
        assert_eq!(p.op, Op::Ne);
    }

    // --- Word operator parsing ---

    #[test]
    fn parse_word_lte() {
        let p = Predicate::parse("input_cost lte 3", false).unwrap();
        assert_eq!(p.field(), Field::InputCost);
        assert_eq!(p.op, Op::Le);
    }

    #[test]
    fn parse_word_contains() {
        let p = Predicate::parse("name contains opus", false).unwrap();
        assert_eq!(p.field(), Field::Name);
        assert_eq!(p.op, Op::Contains);
    }

    #[test]
    fn parse_word_ncontains() {
        let p = Predicate::parse("provider_name ncontains openweights", false).unwrap();
        assert_eq!(p.field(), Field::ProviderName);
        assert_eq!(p.op, Op::NotContains);
    }

    #[test]
    fn parse_word_gt() {
        let p = Predicate::parse("context_limit gt 100000", false).unwrap();
        assert_eq!(p.field(), Field::ContextLimit);
        assert_eq!(p.op, Op::Gt);
    }

    // --- Error cases ---

    #[test]
    fn error_empty() {
        assert!(matches!(
            Predicate::parse("", false),
            Err(FilterError::Empty)
        ));
        assert!(matches!(
            Predicate::parse("   ", false),
            Err(FilterError::Empty)
        ));
    }

    #[test]
    fn error_no_operator() {
        let e = Predicate::parse("name", false);
        assert!(matches!(e, Err(FilterError::NoOperator(_))));
    }

    #[test]
    fn error_unknown_field() {
        let e = Predicate::parse("foobar=value", false);
        assert!(matches!(e, Err(FilterError::UnknownField(_))));
    }

    #[test]
    fn error_not_comparable_bool_ordering() {
        let e = Predicate::parse("reasoning<true", false);
        assert!(
            matches!(e, Err(FilterError::NotComparable { .. })),
            "ordering op on Bool should be NotComparable, got: {e:?}"
        );
    }

    #[test]
    fn error_not_comparable_list_ordering() {
        let e = Predicate::parse("input_modalities<text", false);
        assert!(matches!(e, Err(FilterError::NotComparable { .. })));
    }

    #[test]
    fn error_bad_number() {
        let e = Predicate::parse("input_cost<=abc", false);
        assert!(matches!(e, Err(FilterError::BadNumber(_))));
    }

    // --- Matching: numeric filters ---

    #[test]
    fn input_cost_le_1_excludes_none_and_expensive() {
        let catalog = sample_catalog();
        let p = Predicate::parse("input_cost<=1", false).unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| p.matches(m))
            .collect();
        // haiku (0.25) and gpt-oss (0.0) match; opus (5.0) and qwen (cost=None) don't.
        assert!(
            matching.iter().all(|m| m
                .cost
                .as_ref()
                .and_then(|c| c.input)
                .is_some_and(|v| v <= 1.0)),
            "all matched models must have input_cost <= 1"
        );
        assert!(
            matching.iter().any(|m| m.name == "Test Haiku"),
            "haiku should match"
        );
        assert!(
            !matching.iter().any(|m| m.name == "Test Opus"),
            "opus should not match"
        );
        assert!(
            !matching.iter().any(|m| m.name == "Qwen3 30B"),
            "qwen (no cost) should not match"
        );
    }

    #[test]
    fn context_limit_ge_200000() {
        let catalog = sample_catalog();
        let p = Predicate::parse("context_limit>=200000", false).unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| p.matches(m))
            .collect();
        for m in &matching {
            assert!(
                m.limit.context.unwrap_or(0) >= 200_000,
                "context must be >= 200000"
            );
        }
        // opus (1_000_000), haiku (200_000), qwen (262_144) match; gpt-oss (131_072) doesn't.
        assert_eq!(matching.len(), 3);
    }

    // --- Matching: text filters ---

    #[test]
    fn name_contains_opus_case_insensitive() {
        let catalog = sample_catalog();
        let p = Predicate::parse("name~Opus", false).unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| p.matches(m))
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].name, "Test Opus");
    }

    #[test]
    fn provider_name_contains_anthropic_case_insensitive() {
        let catalog = sample_catalog();
        let p = Predicate::parse("provider_name~Anthropic", false).unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| p.matches(m))
            .collect();
        assert_eq!(matching.len(), 2);
        assert!(matching.iter().all(|m| m.provider_name == "Anthropic Test"));
    }

    #[test]
    fn release_date_ge_lexicographic() {
        let catalog = sample_catalog();
        // Dates after 2025-07-01: qwen (2025-07-30), gpt-oss (2025-08-05), opus (2026-05-28)
        let p = Predicate::parse("release_date>=2025-07-01", false).unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| p.matches(m))
            .collect();
        for m in &matching {
            let rd = m.release_date.as_deref().unwrap_or("");
            assert!(
                rd >= "2025-07-01",
                "release_date {rd} should be >= 2025-07-01"
            );
        }
        // haiku (2025-01-15) should not match
        assert!(!matching.iter().any(|m| m.name == "Test Haiku"));
        assert_eq!(matching.len(), 3);
    }

    // --- Matching: bool filters ---

    #[test]
    fn reasoning_eq_true() {
        let catalog = sample_catalog();
        let p = Predicate::parse("reasoning=true", false).unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| p.matches(m))
            .collect();
        assert!(matching.iter().all(|m| m.reasoning == Some(true)));
        // opus and gpt-oss have reasoning=true
        assert_eq!(matching.len(), 2);
    }

    #[test]
    fn reasoning_eq_false_matches_false_and_none() {
        let catalog = sample_catalog();
        let p = Predicate::parse("reasoning=false", false).unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| p.matches(m))
            .collect();
        // None is treated as false, so haiku (false) and qwen (false) match.
        assert_eq!(matching.len(), 2);
    }

    // --- Matching: regex filter ---

    #[test]
    fn name_regex_contains_opus_or_haiku() {
        let catalog = sample_catalog();
        let p = Predicate::parse("name~opus|haiku", true).unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| p.matches(m))
            .collect();
        assert_eq!(matching.len(), 2);
    }

    #[test]
    fn name_regex_bad_pattern_returns_error() {
        let e = Predicate::parse("name~[invalid", true);
        assert!(matches!(e, Err(FilterError::BadRegex(_))));
    }

    // --- matches_all (AND semantics) ---

    #[test]
    fn matches_all_and_combines_predicates() {
        let catalog = sample_catalog();
        let preds = parse_filters(
            &["reasoning=true".to_string(), "input_cost<=1".to_string()],
            false,
        )
        .unwrap();
        let matching: Vec<_> = all_models(&catalog)
            .into_iter()
            .filter(|m| matches_all(m, &preds))
            .collect();
        // reasoning=true: opus, gpt-oss
        // input_cost<=1:  haiku, gpt-oss
        // intersection:   gpt-oss (reasoning=true, input_cost=0.0)
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].name, "GPT OSS 20B");
    }

    #[test]
    fn parse_filters_propagates_error() {
        let result = parse_filters(
            &["input_cost<=1".to_string(), "foobar=x".to_string()],
            false,
        );
        assert!(matches!(result, Err(FilterError::UnknownField(_))));
    }
}
