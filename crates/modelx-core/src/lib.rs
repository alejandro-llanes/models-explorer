//! `modelx-core` — pure domain model and query engine for modelx.
//!
//! No I/O, no async. See `docs/architecture.md`.

pub mod error;
pub mod field;
pub mod filter;
pub mod model;
pub mod query;
pub mod testkit;

// Flat re-exports so downstream crates can write `modelx_core::Catalog` etc.
pub use error::CoreError;
pub use field::{Field, FieldKind, FieldValue};
pub use filter::{matches_all, parse_filters, FilterError, Op, Predicate};
pub use model::{Catalog, Cost, Limit, Modalities, Model, ModelRef, Provider, ReasoningOption};
pub use query::{run_query, Filters, Query, Sort};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::sample_catalog;

    // --- FieldValue::display tests ---

    #[test]
    fn field_value_bool_true_displays_yes() {
        assert_eq!(FieldValue::Bool(Some(true)).display(), "yes");
    }

    #[test]
    fn field_value_bool_false_displays_no() {
        assert_eq!(FieldValue::Bool(Some(false)).display(), "no");
    }

    #[test]
    fn field_value_bool_none_displays_empty() {
        assert_eq!(FieldValue::Bool(None).display(), "");
    }

    #[test]
    fn field_value_float_none_displays_empty() {
        assert_eq!(FieldValue::Float(None).display(), "");
    }

    #[test]
    fn field_value_float_integer_trims_zeros() {
        assert_eq!(FieldValue::Float(Some(5.0)).display(), "5");
    }

    #[test]
    fn field_value_float_decimal_preserved() {
        assert_eq!(FieldValue::Float(Some(1.25)).display(), "1.25");
    }

    #[test]
    fn field_value_list_comma_joined() {
        let v = FieldValue::List(vec!["text".to_string(), "image".to_string()]);
        assert_eq!(v.display(), "text, image");
    }

    #[test]
    fn field_value_int_some() {
        assert_eq!(FieldValue::Int(Some(1_000_000)).display(), "1000000");
    }

    #[test]
    fn field_value_int_none() {
        assert_eq!(FieldValue::Int(None).display(), "");
    }

    // --- Field::value() roundtrip tests ---

    #[test]
    fn field_reasoning_value_on_reasoning_model() {
        let catalog = sample_catalog();
        let model = catalog.find(&ModelRef {
            provider_id: "provider-a".to_string(),
            model_id: "model-opus".to_string(),
        });
        assert!(model.is_some());
        let v = Field::Reasoning.value(model.unwrap());
        assert_eq!(v, FieldValue::Bool(Some(true)));
        assert_eq!(v.display(), "yes");
    }

    #[test]
    fn field_input_cost_on_model_without_cost() {
        let catalog = sample_catalog();
        let model = catalog.find(&ModelRef {
            provider_id: "provider-b".to_string(),
            model_id: "qwen/qwen3-30b".to_string(),
        });
        assert!(model.is_some());
        let v = Field::InputCost.value(model.unwrap());
        assert_eq!(v, FieldValue::Float(None));
        assert_eq!(v.display(), "");
    }

    #[test]
    fn field_context_limit_value() {
        let catalog = sample_catalog();
        let model = catalog.find(&ModelRef {
            provider_id: "provider-a".to_string(),
            model_id: "model-opus".to_string(),
        });
        let v = Field::ContextLimit.value(model.unwrap());
        assert_eq!(v, FieldValue::Int(Some(1_000_000)));
    }

    // --- Catalog helpers ---

    #[test]
    fn catalog_total_models() {
        let catalog = sample_catalog();
        assert_eq!(catalog.total_models(), 4);
    }

    #[test]
    fn catalog_provider_lookup() {
        let catalog = sample_catalog();
        assert!(catalog.provider("provider-a").is_some());
        assert!(catalog.provider("missing").is_none());
    }

    #[test]
    fn catalog_all_models_count() {
        let catalog = sample_catalog();
        assert_eq!(catalog.all_models().count(), 4);
    }

    #[test]
    fn model_key_roundtrip() {
        let catalog = sample_catalog();
        let model = catalog.all_models().next().unwrap();
        let key = model.key();
        let found = catalog.find(&key);
        assert_eq!(found.map(|m| &m.id), Some(&model.id));
    }
}
