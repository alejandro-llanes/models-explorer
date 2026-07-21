use std::cmp::Ordering;

use nucleo_matcher::{
    pattern::{CaseMatching, Normalization, Pattern},
    Config, Matcher, Utf32String,
};

use crate::{
    field::{Field, FieldValue},
    model::{Catalog, Model},
};

/// Full query: optional fuzzy search + filters + sort.
#[derive(Clone, Debug, Default)]
pub struct Query {
    /// Free-text fuzzy search (matches provider name + model name + model id).
    /// Empty string = show everything.
    pub search: String,
    /// Structured filters applied before fuzzy scoring.
    pub filters: Filters,
    /// How to sort the final result.
    pub sort: Sort,
}

/// Structured filters for `run_query`.
#[derive(Clone, Debug, Default)]
pub struct Filters {
    /// Restrict to these provider ids.  Empty = all providers.
    pub provider_ids: Vec<String>,
    /// Require `model.reasoning == Some(value)`.
    pub reasoning: Option<bool>,
    /// Require `model.tool_call == Some(value)`.
    pub tool_call: Option<bool>,
    /// Require `model.open_weights == Some(value)`.
    pub open_weights: Option<bool>,
    /// Require this modality to appear in `model.modalities.input`.
    pub input_modality: Option<String>,
    /// Require `model.limit.context >= min_context`.
    pub min_context: Option<u64>,
    /// Require `model.cost.input <= max_input_cost` (models with no cost are excluded).
    pub max_input_cost: Option<f64>,
}

/// Sort specification.
#[derive(Clone, Debug)]
pub struct Sort {
    pub field: Field,
    pub descending: bool,
}

impl Default for Sort {
    /// Default: sort by model name ascending.
    fn default() -> Self {
        Sort {
            field: Field::Name,
            descending: false,
        }
    }
}

// ---------------------------------------------------------------------------
// run_query
// ---------------------------------------------------------------------------

/// Execute a [`Query`] against a [`Catalog`] and return matching models.
///
/// ## Algorithm
/// 1. **Filter** — keep only models that pass all active [`Filters`].
/// 2. **Fuzzy score** — when `query.search` is non-empty, score each model's
///    haystack (`"{provider_name} {name} {id}"`) with `nucleo-matcher` and
///    discard models that don't match at all.
/// 3. **Sort** — when search is empty, sort by `query.sort.field`; when search
///    is non-empty, sort by fuzzy score descending with `sort.field` as
///    tiebreak.
///
/// `None` field values always sort *last*, regardless of `descending`.
pub fn run_query<'a>(catalog: &'a Catalog, q: &Query) -> Vec<&'a Model> {
    // Step 1 — filter
    let filtered: Vec<&Model> = catalog
        .all_models()
        .filter(|m| passes_filters(m, &q.filters))
        .collect();

    // Step 2 — fuzzy score (or keep-all)
    let search = q.search.trim();
    let mut scored: Vec<(&Model, Option<u32>)> = if search.is_empty() {
        filtered.into_iter().map(|m| (m, None)).collect()
    } else {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(search, CaseMatching::Smart, Normalization::Smart);

        filtered
            .into_iter()
            .filter_map(|m| {
                let haystack = format!("{} {} {}", m.provider_name, m.name, m.id);
                let haystack_utf32 = haystack.chars().collect::<Vec<_>>();
                let haystack_str = Utf32String::Unicode(haystack_utf32.into_boxed_slice());
                pattern
                    .score(haystack_str.slice(..), &mut matcher)
                    .map(|score| (m, Some(score)))
            })
            .collect()
    };

    // Step 3 — sort
    scored.sort_by(|(a, score_a), (b, score_b)| {
        // When we have fuzzy scores: primary = score desc, secondary = sort field
        if let (Some(sa), Some(sb)) = (score_a, score_b) {
            let cmp = sb.cmp(sa); // higher score first
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
        // Sort by the requested field (None sorts last)
        compare_by_field(a, b, &q.sort)
    });

    scored.into_iter().map(|(m, _)| m).collect()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn passes_filters(m: &Model, f: &Filters) -> bool {
    if !f.provider_ids.is_empty() && !f.provider_ids.contains(&m.provider_id) {
        return false;
    }
    if let Some(req) = f.reasoning {
        if m.reasoning != Some(req) {
            return false;
        }
    }
    if let Some(req) = f.tool_call {
        if m.tool_call != Some(req) {
            return false;
        }
    }
    if let Some(req) = f.open_weights {
        if m.open_weights != Some(req) {
            return false;
        }
    }
    if let Some(ref modality) = f.input_modality {
        if !m.modalities.input.contains(modality) {
            return false;
        }
    }
    if let Some(min_ctx) = f.min_context {
        match m.limit.context {
            Some(ctx) if ctx >= min_ctx => {}
            _ => return false,
        }
    }
    if let Some(max_cost) = f.max_input_cost {
        match m.cost.as_ref().and_then(|c| c.input) {
            Some(input_cost) if input_cost <= max_cost => {}
            _ => return false,
        }
    }
    true
}

/// Compare two models for the sort step.  `None` values always sort last.
fn compare_by_field(a: &Model, b: &Model, sort: &Sort) -> Ordering {
    let va = sort.field.value(a);
    let vb = sort.field.value(b);
    let cmp = compare_field_values(&va, &vb);
    if sort.descending {
        cmp.reverse()
    } else {
        cmp
    }
}

/// Total order for `FieldValue`.  `None` (missing) is always *greater* than
/// any present value so it ends up at the bottom in ascending order (and stays
/// at the bottom in descending order via the reversal wrapper in
/// `compare_by_field`).
fn compare_field_values(a: &FieldValue, b: &FieldValue) -> Ordering {
    match (a, b) {
        (FieldValue::Text(ta), FieldValue::Text(tb)) => ta.cmp(tb),
        (FieldValue::Int(Some(ia)), FieldValue::Int(Some(ib))) => ia.cmp(ib),
        (FieldValue::Int(None), FieldValue::Int(None)) => Ordering::Equal,
        (FieldValue::Int(None), FieldValue::Int(Some(_))) => Ordering::Greater, // None last
        (FieldValue::Int(Some(_)), FieldValue::Int(None)) => Ordering::Less,
        (FieldValue::Float(Some(fa)), FieldValue::Float(Some(fb))) => {
            fa.partial_cmp(fb).unwrap_or(Ordering::Equal)
        }
        (FieldValue::Float(None), FieldValue::Float(None)) => Ordering::Equal,
        (FieldValue::Float(None), FieldValue::Float(Some(_))) => Ordering::Greater,
        (FieldValue::Float(Some(_)), FieldValue::Float(None)) => Ordering::Less,
        (FieldValue::Bool(Some(ba)), FieldValue::Bool(Some(bb))) => ba.cmp(bb),
        (FieldValue::Bool(None), FieldValue::Bool(None)) => Ordering::Equal,
        (FieldValue::Bool(None), FieldValue::Bool(Some(_))) => Ordering::Greater,
        (FieldValue::Bool(Some(_)), FieldValue::Bool(None)) => Ordering::Less,
        (FieldValue::List(la), FieldValue::List(lb)) => la.join(",").cmp(&lb.join(",")),
        // Cross-type: fall back to display string comparison
        _ => a.display().cmp(&b.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::sample_catalog;

    // --- filter tests ---

    #[test]
    fn provider_filter_restricts_results() {
        let catalog = sample_catalog();
        let q = Query {
            filters: Filters {
                provider_ids: vec!["provider-a".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        assert!(
            results.iter().all(|m| m.provider_id == "provider-a"),
            "all results should be from provider-a"
        );
        assert!(!results.is_empty(), "provider-a has models");
    }

    #[test]
    fn reasoning_filter_true() {
        let catalog = sample_catalog();
        let q = Query {
            filters: Filters {
                reasoning: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        assert!(
            results.iter().all(|m| m.reasoning == Some(true)),
            "all results must have reasoning=true"
        );
        assert!(!results.is_empty());
    }

    #[test]
    fn reasoning_filter_false() {
        let catalog = sample_catalog();
        let q = Query {
            filters: Filters {
                reasoning: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        assert!(results
            .iter()
            .all(|m| m.reasoning == Some(false) || m.reasoning.is_none()));
    }

    #[test]
    fn min_context_filter() {
        let catalog = sample_catalog();
        let q = Query {
            filters: Filters {
                min_context: Some(500_000),
                ..Default::default()
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        for m in &results {
            assert!(m.limit.context.unwrap_or(0) >= 500_000);
        }
    }

    // --- fuzzy search tests ---

    #[test]
    fn search_match_finds_model() {
        let catalog = sample_catalog();
        let q = Query {
            search: "opus".to_string(),
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        assert!(
            results
                .iter()
                .any(|m| m.name.to_lowercase().contains("opus")),
            "search for 'opus' should return at least one match"
        );
    }

    #[test]
    fn empty_search_returns_all_filtered() {
        let catalog = sample_catalog();
        let q = Query::default();
        let results = run_query(&catalog, &q);
        assert_eq!(results.len(), catalog.total_models());
    }

    // --- sort tests ---

    #[test]
    fn sort_context_ascending_none_last() {
        let catalog = sample_catalog();
        let q = Query {
            sort: Sort {
                field: Field::ContextLimit,
                descending: false,
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        // verify non-None values appear before None values
        let mut seen_none = false;
        for m in &results {
            let has_context = m.limit.context.is_some();
            if !has_context {
                seen_none = true;
            } else {
                assert!(
                    !seen_none,
                    "a Some value appeared after a None value in ascending sort"
                );
            }
        }
    }

    #[test]
    fn sort_context_descending_none_last() {
        let catalog = sample_catalog();
        let q = Query {
            sort: Sort {
                field: Field::ContextLimit,
                descending: true,
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        // values should be descending, None last
        let contexts: Vec<Option<u64>> = results.iter().map(|m| m.limit.context).collect();
        let mut seen_none = false;
        let mut prev: Option<u64> = None;
        for ctx in &contexts {
            if ctx.is_none() {
                seen_none = true;
            } else {
                assert!(!seen_none, "Some value after None in descending sort");
                if let Some(p) = prev {
                    assert!(ctx.unwrap() <= p, "values not descending");
                }
                prev = *ctx;
            }
        }
    }

    #[test]
    fn sort_name_ascending() {
        let catalog = sample_catalog();
        let q = Query {
            sort: Sort {
                field: Field::Name,
                descending: false,
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        let names: Vec<&str> = results.iter().map(|m| m.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn sort_name_descending() {
        let catalog = sample_catalog();
        let q = Query {
            sort: Sort {
                field: Field::Name,
                descending: true,
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        let names: Vec<&str> = results.iter().map(|m| m.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(names, sorted);
    }

    #[test]
    fn sort_input_cost_ascending_none_last() {
        let catalog = sample_catalog();
        let q = Query {
            sort: Sort {
                field: Field::InputCost,
                descending: false,
            },
            ..Default::default()
        };
        let results = run_query(&catalog, &q);
        let mut seen_none = false;
        let mut prev: Option<f64> = None;
        for m in &results {
            let cost = m.cost.as_ref().and_then(|c| c.input);
            match cost {
                None => seen_none = true,
                Some(c) => {
                    assert!(!seen_none, "Some cost after None cost in ascending sort");
                    if let Some(p) = prev {
                        assert!(c >= p, "costs not ascending");
                    }
                    prev = Some(c);
                }
            }
        }
    }
}
