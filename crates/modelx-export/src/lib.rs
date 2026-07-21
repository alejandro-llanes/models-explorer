//! `modelx-export` — render a model selection to JSON, CSV, Markdown, or a plain list.
//!
//! See `docs/architecture.md`.

use modelx_core::{Field, FieldValue, Model};
use serde_json::{Map, Value};
use std::path::Path;
use thiserror::Error;

/// Output format for an export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    PlainList,
    Csv,
    Markdown,
    Json,
}

static ALL_FORMATS: &[Format] = &[
    Format::PlainList,
    Format::Csv,
    Format::Markdown,
    Format::Json,
];

impl Format {
    /// All supported formats in a stable order.
    pub fn all() -> &'static [Format] {
        ALL_FORMATS
    }

    /// Human-readable label for the format.
    pub fn label(&self) -> &'static str {
        match self {
            Format::PlainList => "Plain list",
            Format::Csv => "CSV",
            Format::Markdown => "Markdown",
            Format::Json => "JSON",
        }
    }

    /// File extension for the format (without the leading dot).
    pub fn ext(&self) -> &'static str {
        match self {
            Format::PlainList => "txt",
            Format::Csv => "csv",
            Format::Markdown => "md",
            Format::Json => "json",
        }
    }
}

/// A request to export a selection of models in a given format.
pub struct ExportRequest<'a> {
    pub models: Vec<&'a Model>,
    pub fields: Vec<Field>,
    pub format: Format,
}

/// Errors that can occur during export.
#[derive(Debug, Error)]
pub enum ExportError {
    /// Returned when `fields` is empty.
    #[error("no fields selected for export")]
    NoFields,

    /// Wraps errors from the `csv` crate.
    #[error("CSV serialisation error: {0}")]
    Csv(#[from] csv::Error),

    /// Wraps errors from `serde_json` (e.g. IntoInnerError when flushing).
    #[error("JSON serialisation error: {0}")]
    Json(#[from] serde_json::Error),

    /// Wraps I/O errors (file writing).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// csv::IntoInnerError wraps the underlying I/O error; we need a separate From impl.
impl From<csv::IntoInnerError<csv::Writer<Vec<u8>>>> for ExportError {
    fn from(e: csv::IntoInnerError<csv::Writer<Vec<u8>>>) -> Self {
        ExportError::Io(e.into_error())
    }
}

/// Render `req` to a `String` in the requested format.
///
/// Returns `Err(ExportError::NoFields)` if `req.fields` is empty.
pub fn render(req: &ExportRequest) -> Result<String, ExportError> {
    if req.fields.is_empty() {
        return Err(ExportError::NoFields);
    }

    match req.format {
        Format::PlainList => render_plain_list(req),
        Format::Csv => render_csv(req),
        Format::Markdown => render_markdown(req),
        Format::Json => render_json(req),
    }
}

/// Render `req` and write the result to `path` (creates or truncates the file).
pub fn write(req: &ExportRequest, path: &Path) -> Result<(), ExportError> {
    let content = render(req)?;
    std::fs::write(path, content.as_bytes())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Format renderers
// ---------------------------------------------------------------------------

fn render_plain_list(req: &ExportRequest) -> Result<String, ExportError> {
    let single = req.fields.len() == 1;
    let mut lines: Vec<String> = Vec::with_capacity(req.models.len());

    for model in &req.models {
        if single {
            lines.push(req.fields[0].value(model).display());
        } else {
            let cells: Vec<String> = req
                .fields
                .iter()
                .map(|f| f.value(model).display())
                .collect();
            lines.push(cells.join("\t"));
        }
    }

    Ok(lines.join("\n"))
}

fn render_csv(req: &ExportRequest) -> Result<String, ExportError> {
    let mut wtr = csv::Writer::from_writer(Vec::new());

    // Header row
    let headers: Vec<&str> = req.fields.iter().map(|f| f.label()).collect();
    wtr.write_record(&headers)?;

    // Data rows
    for model in &req.models {
        let cells: Vec<String> = req
            .fields
            .iter()
            .map(|f| f.value(model).display())
            .collect();
        wtr.write_record(&cells)?;
    }

    let bytes = wtr.into_inner()?;
    Ok(String::from_utf8(bytes).expect("csv output is always valid UTF-8"))
}

fn render_markdown(req: &ExportRequest) -> Result<String, ExportError> {
    let mut out = String::new();

    // Header row
    out.push('|');
    for field in &req.fields {
        out.push(' ');
        out.push_str(field.label());
        out.push_str(" |");
    }
    out.push('\n');

    // Separator row
    out.push('|');
    for _ in &req.fields {
        out.push_str(" --- |");
    }
    out.push('\n');

    // Data rows
    for model in &req.models {
        out.push('|');
        for field in &req.fields {
            let raw = field.value(model).display();
            let cell = raw.replace('\n', " ").replace('|', r"\|");
            out.push(' ');
            out.push_str(&cell);
            out.push_str(" |");
        }
        out.push('\n');
    }

    Ok(out)
}

fn render_json(req: &ExportRequest) -> Result<String, ExportError> {
    let array: Vec<Value> = req
        .models
        .iter()
        .map(|model| {
            let mut obj = Map::new();
            for field in &req.fields {
                let v = field.value(model);
                let json_val = field_value_to_json(&v);
                obj.insert(field.key().to_string(), json_val);
            }
            Value::Object(obj)
        })
        .collect();

    Ok(serde_json::to_string_pretty(&Value::Array(array))?)
}

fn field_value_to_json(v: &FieldValue) -> Value {
    match v {
        FieldValue::Text(s) => Value::String(s.clone()),
        FieldValue::Int(None) => Value::Null,
        FieldValue::Int(Some(n)) => Value::Number((*n).into()),
        FieldValue::Float(None) => Value::Null,
        FieldValue::Float(Some(f)) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        FieldValue::Bool(None) => Value::Null,
        FieldValue::Bool(Some(b)) => Value::Bool(*b),
        FieldValue::List(items) => {
            Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use modelx_core::testkit::sample_catalog;

    /// Pick two models: model-opus (has cost) and qwen/qwen3-30b (cost=None)
    fn two_models(catalog: &modelx_core::Catalog) -> Vec<&Model> {
        vec![
            catalog
                .find(&modelx_core::ModelRef {
                    provider_id: "provider-a".to_string(),
                    model_id: "model-opus".to_string(),
                })
                .unwrap(),
            catalog
                .find(&modelx_core::ModelRef {
                    provider_id: "provider-b".to_string(),
                    model_id: "qwen/qwen3-30b".to_string(),
                })
                .unwrap(),
        ]
    }

    fn three_fields() -> Vec<Field> {
        vec![Field::Id, Field::Name, Field::InputCost]
    }

    // -----------------------------------------------------------------------
    // NoFields guard
    // -----------------------------------------------------------------------

    #[test]
    fn render_no_fields_returns_error() {
        let catalog = sample_catalog();
        let models = two_models(&catalog);
        let req = ExportRequest {
            models,
            fields: vec![],
            format: Format::PlainList,
        };
        assert!(matches!(render(&req), Err(ExportError::NoFields)));
    }

    // -----------------------------------------------------------------------
    // PlainList
    // -----------------------------------------------------------------------

    #[test]
    fn plain_list_single_field_name_only() {
        let catalog = sample_catalog();
        let models = two_models(&catalog);
        let req = ExportRequest {
            models,
            fields: vec![Field::Name],
            format: Format::PlainList,
        };
        let out = render(&req).unwrap();
        assert_eq!(out, "Test Opus\nQwen3 30B");
    }

    #[test]
    fn plain_list_multi_field_tab_separated() {
        let catalog = sample_catalog();
        let models = two_models(&catalog);
        let req = ExportRequest {
            models,
            fields: three_fields(),
            format: Format::PlainList,
        };
        let out = render(&req).unwrap();
        // opus: id=model-opus, name=Test Opus, input_cost=5
        // qwen: id=qwen/qwen3-30b, name=Qwen3 30B, input_cost="" (cost=None)
        let expected = "model-opus\tTest Opus\t5\nqwen/qwen3-30b\tQwen3 30B\t";
        assert_eq!(out, expected);
    }

    // -----------------------------------------------------------------------
    // CSV
    // -----------------------------------------------------------------------

    #[test]
    fn csv_exact_output() {
        let catalog = sample_catalog();
        let models = two_models(&catalog);
        let req = ExportRequest {
            models,
            fields: three_fields(),
            format: Format::Csv,
        };
        let out = render(&req).unwrap();
        // The csv crate uses the platform line ending; normalise for comparison.
        let normalised = out.replace("\r\n", "\n");
        let expected = "ID,Name,Input $/M\nmodel-opus,Test Opus,5\nqwen/qwen3-30b,Qwen3 30B,\n";
        assert_eq!(normalised, expected);
    }

    // -----------------------------------------------------------------------
    // Markdown
    // -----------------------------------------------------------------------

    #[test]
    fn markdown_exact_output() {
        let catalog = sample_catalog();
        let models = two_models(&catalog);
        let req = ExportRequest {
            models,
            fields: three_fields(),
            format: Format::Markdown,
        };
        let out = render(&req).unwrap();
        let expected = "| ID | Name | Input $/M |\n\
                        | --- | --- | --- |\n\
                        | model-opus | Test Opus | 5 |\n\
                        | qwen/qwen3-30b | Qwen3 30B |  |\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn markdown_escapes_pipe_in_cell() {
        let mut model = sample_catalog().all_models().next().unwrap().clone();
        // Artificially inject a | into the name to test escaping.
        model.name = "foo|bar".to_string();

        let req = ExportRequest {
            models: vec![&model],
            fields: vec![Field::Name],
            format: Format::Markdown,
        };
        let out = render(&req).unwrap();
        assert!(out.contains(r"foo\|bar"), "pipe should be escaped: {out}");
    }

    // -----------------------------------------------------------------------
    // JSON
    // -----------------------------------------------------------------------

    #[test]
    fn json_exact_output() {
        let catalog = sample_catalog();
        let models = two_models(&catalog);
        let req = ExportRequest {
            models,
            fields: three_fields(),
            format: Format::Json,
        };
        let out = render(&req).unwrap();

        // Parse back and check structure rather than exact whitespace.
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed.len(), 2);

        let opus = &parsed[0];
        assert_eq!(opus["id"], "model-opus");
        assert_eq!(opus["name"], "Test Opus");
        assert_eq!(opus["input_cost"], 5.0);

        let qwen = &parsed[1];
        assert_eq!(qwen["id"], "qwen/qwen3-30b");
        assert_eq!(qwen["name"], "Qwen3 30B");
        assert!(
            qwen["input_cost"].is_null(),
            "cost=None should be JSON null"
        );
    }

    #[test]
    fn json_typed_values() {
        // Verify that bools come out as JSON bools and lists as JSON arrays.
        let catalog = sample_catalog();
        let opus = catalog
            .find(&modelx_core::ModelRef {
                provider_id: "provider-a".to_string(),
                model_id: "model-opus".to_string(),
            })
            .unwrap();

        let req = ExportRequest {
            models: vec![opus],
            fields: vec![Field::Reasoning, Field::InputModalities],
            format: Format::Json,
        };
        let out = render(&req).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        let obj = &parsed[0];
        assert_eq!(obj["reasoning"], true);
        assert_eq!(
            obj["input_modalities"],
            serde_json::json!(["text", "image"])
        );
    }

    // -----------------------------------------------------------------------
    // write() round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn write_round_trips_to_tempfile() {
        let catalog = sample_catalog();
        let models = two_models(&catalog);
        let req = ExportRequest {
            models,
            fields: vec![Field::Name],
            format: Format::PlainList,
        };

        let tmp = tempfile::NamedTempFile::new().unwrap();
        write(&req, tmp.path()).unwrap();

        let content = std::fs::read_to_string(tmp.path()).unwrap();
        assert_eq!(content, "Test Opus\nQwen3 30B");
    }
}
