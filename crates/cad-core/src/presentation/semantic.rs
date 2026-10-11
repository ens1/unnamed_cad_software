use super::SemanticElement;
use serde::{Deserialize, Serialize};

/// Renderer-independent value and editing information. Geometry and document
/// mutations still travel through the element's command, never through styling.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ValueKind {
    Text,
    Boolean,
    Number,
    Quantity,
    Choice,
    Reference,
    Json,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValueDescriptor {
    pub kind: ValueKind,
    pub value: serde_json::Value,
    pub draft_text: Option<String>,
    pub unit: Option<String>,
    pub dimension: Option<serde_json::Value>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub choices: Vec<serde_json::Value>,
    pub editable: bool,
    pub edit_command: Option<String>,
    pub accessibility_label: String,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionDescriptor {
    pub id: String,
    pub command: String,
    pub label: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub arguments: serde_json::Value,
    pub accessibility_label: String,
}
impl SemanticElement {
    /// Supports existing projections while letting future publishers provide the
    /// richer `data.value_descriptor` and `data.actions` contracts directly.
    pub fn value_descriptor(&self) -> ValueDescriptor {
        let data = self.data.get("value_descriptor").unwrap_or(&self.data);
        let json = data
            .get("json")
            .or_else(|| data.get("value"))
            .cloned()
            .unwrap_or_else(|| self.value.clone().map(Into::into).unwrap_or_default());
        let kind = match data
            .get("kind")
            .or_else(|| data.get("value_type"))
            .or_else(|| data.get("type"))
            .and_then(|value| value.as_str())
        {
            Some("boolean") => ValueKind::Boolean,
            Some("number") => ValueKind::Number,
            Some("quantity" | "expression") => ValueKind::Quantity,
            Some("choice" | "enum") => ValueKind::Choice,
            Some("reference") => ValueKind::Reference,
            Some("json") => ValueKind::Json,
            _ if json.is_boolean() => ValueKind::Boolean,
            _ if json.is_number() => ValueKind::Number,
            _ => ValueKind::Text,
        };
        let string = |key| data.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        ValueDescriptor {
            kind,
            value: json,
            draft_text: string("draft_text").or_else(|| self.value.clone()),
            unit: string("unit").or_else(|| string("units")),
            dimension: data.get("dimension").cloned(),
            minimum: data
                .get("minimum")
                .or_else(|| data.get("min"))
                .and_then(|v| v.as_f64()),
            maximum: data
                .get("maximum")
                .or_else(|| data.get("max"))
                .and_then(|v| v.as_f64()),
            choices: data
                .get("choices")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
            editable: data
                .get("editable")
                .and_then(|v| v.as_bool())
                .unwrap_or_else(|| {
                    self.commands
                        .iter()
                        .any(|command| command.ends_with(".edit"))
                }),
            edit_command: string("edit_command").or_else(|| {
                self.commands
                    .iter()
                    .find(|command| command.ends_with(".edit"))
                    .cloned()
            }),
            accessibility_label: string("accessibility_label")
                .unwrap_or_else(|| self.label.clone()),
            diagnostics: data
                .get("diagnostics")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
        }
    }
    pub fn action_descriptors(&self) -> Vec<ActionDescriptor> {
        let mut commands = self.commands.clone();
        for command in self
            .data
            .get("actions")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|action| action.get("command").and_then(|value| value.as_str()))
        {
            if !commands.iter().any(|existing| existing == command) {
                commands.push(command.into());
            }
        }
        commands
            .iter()
            .map(|command| {
                let published = self
                    .data
                    .get("actions")
                    .and_then(|v| v.as_array())
                    .and_then(|actions| {
                        actions.iter().find(|action| {
                            action.get("command").and_then(|v| v.as_str()) == Some(command)
                        })
                    });
                let enabled = published
                    .and_then(|v| v.get("enabled"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(self.state != "disabled");
                let label = published
                    .and_then(|v| v.get("label"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&self.label)
                    .to_owned();
                ActionDescriptor {
                    id: format!("{}::action:{command}", self.id),
                    command: command.clone(),
                    label: label.clone(),
                    enabled,
                    disabled_reason: (!enabled)
                        .then(|| {
                            published
                                .and_then(|v| v.get("disabled_reason"))
                                .and_then(|v| v.as_str())
                                .map(str::to_owned)
                                .or_else(|| self.value.clone())
                        })
                        .flatten(),
                    arguments: published
                        .and_then(|v| v.get("arguments"))
                        .cloned()
                        .unwrap_or_else(|| self.data.clone()),
                    accessibility_label: published
                        .and_then(|v| v.get("accessibility_label"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&label)
                        .to_owned(),
                }
            })
            .collect()
    }
}
