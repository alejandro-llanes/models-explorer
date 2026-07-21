//! Integration tests for `parse_catalog` against the committed fixture.
//!
//! No network access is used here; `fetch()` is covered by an `#[ignore]`d test.

use modelx_datasource::parse_catalog;

static FIXTURE: &[u8] = include_bytes!("fixtures/api-sample.json");

#[test]
fn provider_count_is_4() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    assert_eq!(catalog.providers.len(), 4);
}

#[test]
fn total_model_count_is_7() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    assert_eq!(catalog.total_models(), 7);
}

#[test]
fn null_reasoning_option_values_are_dropped() {
    // Regression: the live models.dev API sometimes includes a `null` element
    // inside a reasoning_options `values` array. Parsing must tolerate it and
    // drop the nulls instead of failing.
    let json = br#"{
      "acme": {
        "id": "acme",
        "name": "Acme",
        "models": {
          "m1": {
            "id": "m1",
            "name": "Model One",
            "reasoning_options": [
              { "type": "effort", "values": [null, "low", "high"] }
            ]
          }
        }
      }
    }"#;
    let catalog = parse_catalog(json).expect("parse should tolerate null values");
    let model = catalog.all_models().next().expect("one model expected");
    let opt = &model.reasoning_options[0];
    assert_eq!(opt.kind, "effort");
    assert_eq!(opt.values, vec!["low".to_string(), "high".to_string()]);
}

#[test]
fn anthropic_provider_present_with_denormalized_name() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    let anthropic = catalog.provider("anthropic").expect("anthropic not found");
    assert_eq!(anthropic.id, "anthropic");
    // Every model under anthropic must have provider_id and provider_name set.
    for model in &anthropic.models {
        assert_eq!(model.provider_id, "anthropic");
        assert_eq!(model.provider_name, "Anthropic");
    }
}

#[test]
fn at_least_one_deprecated_model() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    let deprecated = catalog
        .all_models()
        .any(|m| m.status.as_deref() == Some("deprecated"));
    assert!(
        deprecated,
        "expected at least one model with status=deprecated"
    );
}

#[test]
fn xai_model_has_tiers_and_context_over_200k() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    let xai = catalog.provider("xai").expect("xai provider not found");
    let grok = xai
        .models
        .iter()
        .find(|m| m.id == "grok-4.20-0309-reasoning")
        .expect("grok model not found");
    let cost = grok.cost.as_ref().expect("grok model has no cost");
    assert!(cost.tiers.is_some(), "expected cost.tiers to be Some");
    assert!(
        cost.context_over_200k.is_some(),
        "expected cost.context_over_200k to be Some"
    );
}

#[test]
fn reasoning_options_kind_is_captured() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    // claude-opus-4-8 has a reasoning_option with type="effort"
    let anthropic = catalog.provider("anthropic").expect("anthropic not found");
    let opus = anthropic
        .models
        .iter()
        .find(|m| m.id == "claude-opus-4-8")
        .expect("claude-opus-4-8 not found");
    assert!(
        !opus.reasoning_options.is_empty(),
        "expected reasoning_options to be non-empty"
    );
    assert_eq!(
        opus.reasoning_options[0].kind, "effort",
        "expected kind='effort', got '{}'",
        opus.reasoning_options[0].kind
    );
}

#[test]
fn every_model_raw_is_json_object() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    for model in catalog.all_models() {
        assert!(
            model.raw.is_object(),
            "model '{}' has non-object raw: {:?}",
            model.id,
            model.raw
        );
    }
}

#[test]
fn source_id_is_models_dev() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    assert_eq!(catalog.source_id, "models.dev");
}

#[test]
fn fetched_at_is_none() {
    let catalog = parse_catalog(FIXTURE).expect("parse failed");
    assert!(catalog.fetched_at.is_none());
}

/// Verify the live network endpoint — skipped by default; run with:
/// `cargo test -p modelx-datasource -- --ignored`
#[test]
#[ignore]
fn fetch_live_network() {
    use modelx_datasource::DataSource;
    use modelx_datasource::ModelsDevSource;

    let source = ModelsDevSource::new();
    let catalog = source.fetch().expect("live fetch failed");
    assert!(!catalog.providers.is_empty());
    assert!(catalog.total_models() > 0);
}
