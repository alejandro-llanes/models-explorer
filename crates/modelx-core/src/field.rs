use crate::model::Model;

/// The semantic kind of a [`Field`], used by the filter engine to determine
/// how to parse and compare values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// Comparable as a number (integer or float).
    Number,
    /// A boolean flag (`true`/`false`/`yes`/`no`/`1`/`0`).
    Bool,
    /// An ordered list of strings (modalities, reasoning effort levels).
    List,
    /// Plain text, including dates and knowledge cutoffs (compared lexicographically).
    Text,
}

/// Every displayable / sortable / exportable field of a [`Model`].
///
/// This registry is the single source of truth for columns, the detail pane,
/// export field-selection, and sort keys — add a variant here and it appears
/// everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    ProviderId,
    ProviderName,
    Id,
    Name,
    Description,
    Family,
    Status,
    ContextLimit,
    OutputLimit,
    InputCost,
    OutputCost,
    CacheReadCost,
    CacheWriteCost,
    ReasoningCost,
    Reasoning,
    ToolCall,
    StructuredOutput,
    Attachment,
    Temperature,
    OpenWeights,
    Knowledge,
    ReleaseDate,
    LastUpdated,
    InputModalities,
    OutputModalities,
    ReasoningEfforts,
}

/// All known fields in a stable, canonical order.
static ALL_FIELDS: &[Field] = &[
    Field::ProviderId,
    Field::ProviderName,
    Field::Id,
    Field::Name,
    Field::Description,
    Field::Family,
    Field::Status,
    Field::ContextLimit,
    Field::OutputLimit,
    Field::InputCost,
    Field::OutputCost,
    Field::CacheReadCost,
    Field::CacheWriteCost,
    Field::ReasoningCost,
    Field::Reasoning,
    Field::ToolCall,
    Field::StructuredOutput,
    Field::Attachment,
    Field::Temperature,
    Field::OpenWeights,
    Field::Knowledge,
    Field::ReleaseDate,
    Field::LastUpdated,
    Field::InputModalities,
    Field::OutputModalities,
    Field::ReasoningEfforts,
];

impl Field {
    /// All fields in canonical order.
    pub fn all() -> &'static [Field] {
        ALL_FIELDS
    }

    /// Parse a [`Field`] from its stable machine key (the inverse of [`Field::key`]).
    ///
    /// Used by the CLI (`--fields id,name`) and config parsing. Returns `None`
    /// for an unknown key.
    pub fn from_key(s: &str) -> Option<Field> {
        Field::all().iter().copied().find(|f| f.key() == s)
    }

    /// Whether this field holds a numeric value (i.e. comparable as a number).
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Field::ContextLimit
                | Field::OutputLimit
                | Field::InputCost
                | Field::OutputCost
                | Field::CacheReadCost
                | Field::CacheWriteCost
                | Field::ReasoningCost
        )
    }

    /// Whether this field is a per-million-token price (rendered with `$`).
    pub fn is_cost(&self) -> bool {
        matches!(
            self,
            Field::InputCost
                | Field::OutputCost
                | Field::CacheReadCost
                | Field::CacheWriteCost
                | Field::ReasoningCost
        )
    }

    /// The semantic kind of this field, used by the filter engine.
    pub fn kind(&self) -> FieldKind {
        if self.is_numeric() {
            return FieldKind::Number;
        }
        match self {
            Field::Reasoning
            | Field::ToolCall
            | Field::StructuredOutput
            | Field::Attachment
            | Field::Temperature
            | Field::OpenWeights => FieldKind::Bool,
            Field::InputModalities | Field::OutputModalities | Field::ReasoningEfforts => {
                FieldKind::List
            }
            _ => FieldKind::Text,
        }
    }

    /// The numeric fields, in canonical order.
    pub fn numeric() -> Vec<Field> {
        Field::all()
            .iter()
            .copied()
            .filter(|f| f.is_numeric())
            .collect()
    }

    /// Stable machine key used for export headers, sort params, config, etc.
    pub fn key(&self) -> &'static str {
        match self {
            Field::ProviderId => "provider_id",
            Field::ProviderName => "provider_name",
            Field::Id => "id",
            Field::Name => "name",
            Field::Description => "description",
            Field::Family => "family",
            Field::Status => "status",
            Field::ContextLimit => "context_limit",
            Field::OutputLimit => "output_limit",
            Field::InputCost => "input_cost",
            Field::OutputCost => "output_cost",
            Field::CacheReadCost => "cache_read_cost",
            Field::CacheWriteCost => "cache_write_cost",
            Field::ReasoningCost => "reasoning_cost",
            Field::Reasoning => "reasoning",
            Field::ToolCall => "tool_call",
            Field::StructuredOutput => "structured_output",
            Field::Attachment => "attachment",
            Field::Temperature => "temperature",
            Field::OpenWeights => "open_weights",
            Field::Knowledge => "knowledge",
            Field::ReleaseDate => "release_date",
            Field::LastUpdated => "last_updated",
            Field::InputModalities => "input_modalities",
            Field::OutputModalities => "output_modalities",
            Field::ReasoningEfforts => "reasoning_efforts",
        }
    }

    /// Human-readable column label.
    pub fn label(&self) -> &'static str {
        match self {
            Field::ProviderId => "Provider ID",
            Field::ProviderName => "Provider",
            Field::Id => "ID",
            Field::Name => "Name",
            Field::Description => "Description",
            Field::Family => "Family",
            Field::Status => "Status",
            Field::ContextLimit => "Context",
            Field::OutputLimit => "Output",
            Field::InputCost => "Input $/M",
            Field::OutputCost => "Output $/M",
            Field::CacheReadCost => "Cache Read $/M",
            Field::CacheWriteCost => "Cache Write $/M",
            Field::ReasoningCost => "Reasoning $/M",
            Field::Reasoning => "Reasoning",
            Field::ToolCall => "Tool Call",
            Field::StructuredOutput => "Structured Output",
            Field::Attachment => "Attachment",
            Field::Temperature => "Temperature",
            Field::OpenWeights => "Open Weights",
            Field::Knowledge => "Knowledge",
            Field::ReleaseDate => "Release Date",
            Field::LastUpdated => "Last Updated",
            Field::InputModalities => "Input Modalities",
            Field::OutputModalities => "Output Modalities",
            Field::ReasoningEfforts => "Reasoning Efforts",
        }
    }

    /// Extract a typed [`FieldValue`] from a [`Model`].
    pub fn value(&self, m: &Model) -> FieldValue {
        match self {
            Field::ProviderId => FieldValue::Text(m.provider_id.clone()),
            Field::ProviderName => FieldValue::Text(m.provider_name.clone()),
            Field::Id => FieldValue::Text(m.id.clone()),
            Field::Name => FieldValue::Text(m.name.clone()),
            Field::Description => FieldValue::Text(m.description.clone()),
            Field::Family => FieldValue::Text(m.family.clone().unwrap_or_default()),
            Field::Status => FieldValue::Text(m.status.clone().unwrap_or_default()),
            Field::ContextLimit => FieldValue::Int(m.limit.context.map(|v| v as i64)),
            Field::OutputLimit => FieldValue::Int(m.limit.output.map(|v| v as i64)),
            Field::InputCost => FieldValue::Float(m.cost.as_ref().and_then(|c| c.input)),
            Field::OutputCost => FieldValue::Float(m.cost.as_ref().and_then(|c| c.output)),
            Field::CacheReadCost => FieldValue::Float(m.cost.as_ref().and_then(|c| c.cache_read)),
            Field::CacheWriteCost => FieldValue::Float(m.cost.as_ref().and_then(|c| c.cache_write)),
            Field::ReasoningCost => FieldValue::Float(m.cost.as_ref().and_then(|c| c.reasoning)),
            Field::Reasoning => FieldValue::Bool(m.reasoning),
            Field::ToolCall => FieldValue::Bool(m.tool_call),
            Field::StructuredOutput => FieldValue::Bool(m.structured_output),
            Field::Attachment => FieldValue::Bool(m.attachment),
            Field::Temperature => FieldValue::Bool(m.temperature),
            Field::OpenWeights => FieldValue::Bool(m.open_weights),
            Field::Knowledge => FieldValue::Text(m.knowledge.clone().unwrap_or_default()),
            Field::ReleaseDate => FieldValue::Text(m.release_date.clone().unwrap_or_default()),
            Field::LastUpdated => FieldValue::Text(m.last_updated.clone().unwrap_or_default()),
            Field::InputModalities => FieldValue::List(m.modalities.input.clone()),
            Field::OutputModalities => FieldValue::List(m.modalities.output.clone()),
            Field::ReasoningEfforts => {
                let efforts: Vec<String> = m
                    .reasoning_options
                    .iter()
                    .flat_map(|ro| ro.values.iter().cloned())
                    .collect();
                FieldValue::List(efforts)
            }
        }
    }
}

/// A typed value extracted from a [`Model`] by a [`Field`].
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Text(String),
    Int(Option<i64>),
    Float(Option<f64>),
    Bool(Option<bool>),
    List(Vec<String>),
}

impl FieldValue {
    /// Human-readable string for display in tables and exports.
    ///
    /// Rules:
    /// - `None` variants → `""`
    /// - `Bool(Some(true))` → `"yes"`, `Bool(Some(false))` → `"no"`
    /// - `List` → comma-joined
    /// - `Float` → trailing zeros trimmed (e.g. `5`, `1.25`)
    /// - `Int` and `Text` → natural string representation
    pub fn display(&self) -> String {
        match self {
            FieldValue::Text(s) => s.clone(),
            FieldValue::Int(None) => String::new(),
            FieldValue::Int(Some(n)) => n.to_string(),
            FieldValue::Float(None) => String::new(),
            FieldValue::Float(Some(f)) => trim_float(*f),
            FieldValue::Bool(None) => String::new(),
            FieldValue::Bool(Some(true)) => "yes".to_string(),
            FieldValue::Bool(Some(false)) => "no".to_string(),
            FieldValue::List(items) => items.join(", "),
        }
    }

    /// The value as `f64`, if it is a present number (`Int`/`Float`).
    ///
    /// `Bool`/`Text`/`List` and any `None` yield `None`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            FieldValue::Int(Some(n)) => Some(*n as f64),
            FieldValue::Float(Some(f)) => Some(*f),
            _ => None,
        }
    }
}

/// Format an f64 without unnecessary trailing zeros.
///
/// Examples: `5.0` → `"5"`, `1.25` → `"1.25"`, `0.5` → `"0.5"`.
fn trim_float(f: f64) -> String {
    // Use a precision high enough for pricing values but strip trailing zeros.
    let s = format!("{f:.10}");
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_float_integer() {
        assert_eq!(trim_float(5.0), "5");
    }

    #[test]
    fn trim_float_decimal() {
        assert_eq!(trim_float(1.25), "1.25");
    }

    #[test]
    fn trim_float_small() {
        assert_eq!(trim_float(0.5), "0.5");
    }
}
