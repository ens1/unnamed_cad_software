//! Versioned discovery of actual typed expression fields in native operations.
//!
//! This adapter preserves the document's ordinary JSON representation. Serde's
//! existing `Expr` newtype marker lets it distinguish expressions from names,
//! references, modes and literal text without maintaining a second feature list.
use crate::{CadError, Expr, Feature, Result};
use serde::{Serialize, Serializer, ser};
use serde_json::{Value, value::Serializer as JsonSerializer};
use std::collections::{BTreeMap, BTreeSet};

pub const FIELD_CONTRACT_VERSION: u32 = 3;
const MARKER: &str = "__tcad_expression_field_v1";
const INTEGER: &str = "__tcad_integer_field_v1";

#[derive(Clone, Copy, Debug, Default, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    #[default]
    Expression,
    Count,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize, PartialEq)]
pub struct DimensionField {
    pub version: u32,
    pub feature: crate::Id,
    /// Operation-relative field path. Array records use `@id=AUTHORED_ID`
    /// segments; ordinary segments retain RFC 6901 escaping.
    pub path: String,
    /// Current resolved JSON pointer; this is projection metadata, not identity.
    #[serde(default)]
    pub pointer: String,
    /// An anonymous array slot appears in the path, so its authoring position matters.
    #[serde(default)]
    pub positional: bool,
    pub label: String,
    pub expression: Expr,
    #[serde(default)]
    pub kind: FieldKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<u64>,
}

pub fn fields(feature: &Feature) -> Result<Vec<DimensionField>> {
    let tagged = feature.operation.serialize(ExpressionSerializer)?;
    let operation = serde_json::to_value(&feature.operation)?;
    let mut expressions = BTreeMap::new();
    collect(&tagged, "", &mut expressions)?;
    let mut fields = expressions
        .into_iter()
        .map(|(path, expression)| {
            let (pointer, positional) = resolve_pointer(&operation, &path)?;
            Ok(DimensionField {
                version: FIELD_CONTRACT_VERSION,
                feature: feature.id.clone(),
                label: format!("{} · {}", feature.name, label(&pointer)),
                path,
                pointer,
                positional,
                expression,
                kind: FieldKind::Expression,
                maximum: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut integers = BTreeMap::new();
    collect_integers(&tagged, "", None, &mut integers)?;
    for (path, (value, maximum)) in integers {
        let (pointer, positional) = resolve_pointer(&operation, &path)?;
        fields.push(DimensionField {
            version: FIELD_CONTRACT_VERSION,
            feature: feature.id.clone(),
            label: format!("{} · {} count", feature.name, label(&pointer)),
            path,
            pointer,
            positional,
            expression: Expr(value.to_string()),
            kind: FieldKind::Count,
            maximum: Some(maximum),
        });
    }
    fields.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(fields)
}

pub fn authored_value(field: &DimensionField, expression: &Expr) -> Result<Value> {
    if field.kind == FieldKind::Expression {
        return Ok(serde_json::to_value(expression)?);
    }
    let text = expression.0.trim();
    let value = text
        .parse::<u64>()
        .map_err(|_| invalid("Count overrides require an unsigned integer literal"))?;
    if field.maximum.is_some_and(|maximum| value > maximum) {
        return Err(invalid("Count exceeds its native integer type"));
    }
    Ok(Value::from(value))
}
fn collect_integers(
    value: &Value,
    path: &str,
    field: Option<&str>,
    out: &mut BTreeMap<String, (u64, u64)>,
) -> Result<()> {
    match value {
        Value::Object(object) if object.len() == 1 && object.contains_key(INTEGER) => {
            // Engineering repetition counts are configurable; schema versions,
            // reference indices and adapter limits are not dimension intent.
            if field.is_some_and(|field| {
                matches!(
                    field,
                    "count" | "counts" | "copies" | "instances" | "sides" | "rows" | "columns"
                )
            }) && let (Some(value), Some(maximum)) = (
                object[INTEGER]["value"].as_u64(),
                object[INTEGER]["maximum"].as_u64(),
            ) {
                out.insert(path.into(), (value, maximum));
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                collect_integers(value, &format!("{path}/{}", escape(key)), Some(key), out)?;
            }
        }
        Value::Array(array) => {
            let mut identities = BTreeSet::new();
            for (index, value) in array.iter().enumerate() {
                let segment = if let Some(id) = value.get("id") {
                    let id = id
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| invalid("Array identities must be nonempty strings"))?;
                    if !identities.insert(id) {
                        return Err(invalid(
                            "Duplicate authored identity in a count record array",
                        ));
                    }
                    format!("@id={}", escape(id))
                } else {
                    index.to_string()
                };
                collect_integers(value, &format!("{path}/{segment}"), field, out)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn label(path: &str) -> String {
    path.trim_start_matches('/')
        .split('/')
        .map(|part| part.replace("~1", "/").replace("~0", "~").replace('_', " "))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn collect(value: &Value, path: &str, out: &mut BTreeMap<String, Expr>) -> Result<()> {
    match value {
        Value::Object(object) if object.len() == 1 && object.contains_key(MARKER) => {
            if let Some(text) = object[MARKER].as_str() {
                out.insert(path.into(), Expr(text.into()));
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                let key = key.replace('~', "~0").replace('/', "~1");
                collect(value, &format!("{path}/{key}"), out)?;
            }
        }
        Value::Array(array) => {
            let mut identities = BTreeSet::new();
            for (index, value) in array.iter().enumerate() {
                let segment = if let Some(id) = value.get("id") {
                    let id = id.as_str().filter(|id| !id.is_empty()).ok_or_else(|| {
                        invalid("Array record identities must be nonempty strings")
                    })?;
                    if !identities.insert(id) {
                        return Err(invalid(
                            "Duplicate authored identity in a dimensional record array",
                        ));
                    }
                    format!("@id={}", escape(id))
                } else {
                    index.to_string()
                };
                collect(value, &format!("{path}/{segment}"), out)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Configuration dimension: {message}"))
}
fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}
fn decode(segment: &str) -> Result<String> {
    let mut decoded = String::new();
    let mut chars = segment.chars();
    while let Some(character) = chars.next() {
        if character != '~' {
            decoded.push(character);
            continue;
        }
        decoded.push(match chars.next() {
            Some('0') => '~',
            Some('1') => '/',
            _ => {
                return Err(invalid(
                    "Invalid RFC 6901 escape in a dimensional field path",
                ));
            }
        });
    }
    Ok(decoded)
}

/// Resolve a stable native field path against the current operation source.
/// Numeric slots are admitted only for records without authored identities.
pub fn resolve_pointer(operation: &Value, path: &str) -> Result<(String, bool)> {
    let segments = path
        .strip_prefix('/')
        .filter(|path| !path.is_empty())
        .ok_or_else(|| invalid("A dimensional field path must begin with /"))?;
    let mut current = operation;
    let mut pointer = String::new();
    let mut positional = false;
    for encoded in segments.split('/') {
        let segment = decode(encoded)?;
        match current {
            Value::Object(object) => {
                current = object
                    .get(&segment)
                    .ok_or_else(|| invalid("The dimensional source field no longer exists"))?;
                pointer.push('/');
                pointer.push_str(&escape(&segment));
            }
            Value::Array(array) => {
                let index = if let Some(id) = segment.strip_prefix("@id=") {
                    let mut matching = array
                        .iter()
                        .enumerate()
                        .filter(|(_, record)| record.get("id").and_then(Value::as_str) == Some(id));
                    let (index, _) = matching.next().ok_or_else(|| {
                        invalid("The authored dimensional record no longer exists")
                    })?;
                    if matching.next().is_some() {
                        return Err(invalid(
                            "The authored dimensional record identity is ambiguous",
                        ));
                    }
                    index
                } else {
                    let index: usize = segment.parse().map_err(|_| {
                        invalid("An anonymous dimensional array requires a numeric slot")
                    })?;
                    if segment != index.to_string() {
                        return Err(invalid(
                            "Dimensional array slots must use canonical unsigned indices",
                        ));
                    }
                    let record = array
                        .get(index)
                        .ok_or_else(|| invalid("The dimensional array slot no longer exists"))?;
                    if record.get("id").is_some() {
                        return Err(invalid(
                            "A record with an authored identity requires an @id= field selector; replace its positional override",
                        ));
                    }
                    positional = true;
                    index
                };
                current = &array[index];
                pointer.push('/');
                pointer.push_str(&index.to_string());
            }
            _ => {
                return Err(invalid(
                    "The dimensional field path crosses a scalar source value",
                ));
            }
        }
    }
    Ok((pointer, positional))
}

struct Captured<'a, T: ?Sized>(&'a T);
impl<T: Serialize + ?Sized> Serialize for Captured<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.0
            .serialize(ExpressionSerializer)
            .map_err(ser::Error::custom)?
            .serialize(serializer)
    }
}
struct Compound<T>(T);
struct ExpressionSerializer;

macro_rules! scalar {
    ($($method:ident: $ty:ty),* $(,)?) => {$(
        fn $method(self, value: $ty) -> std::result::Result<Value, Self::Error> {
            JsonSerializer.$method(value)
        }
    )*};
}
impl Serializer for ExpressionSerializer {
    type Ok = Value;
    type Error = serde_json::Error;
    type SerializeSeq = Compound<<JsonSerializer as Serializer>::SerializeSeq>;
    type SerializeTuple = Compound<<JsonSerializer as Serializer>::SerializeTuple>;
    type SerializeTupleStruct = Compound<<JsonSerializer as Serializer>::SerializeTupleStruct>;
    type SerializeTupleVariant = Compound<<JsonSerializer as Serializer>::SerializeTupleVariant>;
    type SerializeMap = Compound<<JsonSerializer as Serializer>::SerializeMap>;
    type SerializeStruct = Compound<<JsonSerializer as Serializer>::SerializeStruct>;
    type SerializeStructVariant = Compound<<JsonSerializer as Serializer>::SerializeStructVariant>;
    scalar!(serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_i128: i128, serialize_u128: u128, serialize_f32: f32,
        serialize_f64: f64, serialize_char: char, serialize_str: &str, serialize_bytes: &[u8]);
    fn serialize_u8(self, value: u8) -> std::result::Result<Value, Self::Error> {
        Ok(serde_json::json!({(INTEGER):{"value":value,"maximum":u8::MAX}}))
    }
    fn serialize_u16(self, value: u16) -> std::result::Result<Value, Self::Error> {
        Ok(serde_json::json!({(INTEGER):{"value":value,"maximum":u16::MAX}}))
    }
    fn serialize_u32(self, value: u32) -> std::result::Result<Value, Self::Error> {
        Ok(serde_json::json!({(INTEGER):{"value":value,"maximum":u32::MAX}}))
    }
    fn serialize_u64(self, value: u64) -> std::result::Result<Value, Self::Error> {
        Ok(serde_json::json!({(INTEGER):{"value":value,"maximum":u64::MAX}}))
    }
    fn serialize_none(self) -> std::result::Result<Value, Self::Error> {
        JsonSerializer.serialize_none()
    }
    fn serialize_some<T: Serialize + ?Sized>(
        self,
        value: &T,
    ) -> std::result::Result<Value, Self::Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> std::result::Result<Value, Self::Error> {
        JsonSerializer.serialize_unit()
    }
    fn serialize_unit_struct(self, name: &'static str) -> std::result::Result<Value, Self::Error> {
        JsonSerializer.serialize_unit_struct(name)
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> std::result::Result<Value, Self::Error> {
        JsonSerializer.serialize_unit_variant(name, index, variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> std::result::Result<Value, Self::Error> {
        if name == "Expr" {
            let mut object = serde_json::Map::new();
            object.insert(MARKER.into(), serde_json::to_value(value)?);
            Ok(Value::Object(object))
        } else {
            value.serialize(self)
        }
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> std::result::Result<Value, Self::Error> {
        JsonSerializer.serialize_newtype_variant(name, index, variant, &Captured(value))
    }
    fn serialize_seq(
        self,
        len: Option<usize>,
    ) -> std::result::Result<Self::SerializeSeq, Self::Error> {
        JsonSerializer.serialize_seq(len).map(Compound)
    }
    fn serialize_tuple(self, len: usize) -> std::result::Result<Self::SerializeTuple, Self::Error> {
        JsonSerializer.serialize_tuple(len).map(Compound)
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> std::result::Result<Self::SerializeTupleStruct, Self::Error> {
        JsonSerializer
            .serialize_tuple_struct(name, len)
            .map(Compound)
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> std::result::Result<Self::SerializeTupleVariant, Self::Error> {
        JsonSerializer
            .serialize_tuple_variant(name, index, variant, len)
            .map(Compound)
    }
    fn serialize_map(
        self,
        len: Option<usize>,
    ) -> std::result::Result<Self::SerializeMap, Self::Error> {
        JsonSerializer.serialize_map(len).map(Compound)
    }
    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> std::result::Result<Self::SerializeStruct, Self::Error> {
        JsonSerializer.serialize_struct(name, len).map(Compound)
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> std::result::Result<Self::SerializeStructVariant, Self::Error> {
        JsonSerializer
            .serialize_struct_variant(name, index, variant, len)
            .map(Compound)
    }
}

macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl<T: ser::$trait<Ok = Value, Error = serde_json::Error>> ser::$trait for Compound<T> {
            type Ok = Value;
            type Error = serde_json::Error;
            fn $method<V: Serialize + ?Sized>(
                &mut self,
                value: &V,
            ) -> std::result::Result<(), Self::Error> {
                self.0.$method(&Captured(value))
            }
            fn end(self) -> std::result::Result<Value, Self::Error> {
                self.0.end()
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);
impl<T: ser::SerializeMap<Ok = Value, Error = serde_json::Error>> ser::SerializeMap
    for Compound<T>
{
    type Ok = Value;
    type Error = serde_json::Error;
    fn serialize_key<K: Serialize + ?Sized>(
        &mut self,
        key: &K,
    ) -> std::result::Result<(), Self::Error> {
        self.0.serialize_key(key)
    }
    fn serialize_value<V: Serialize + ?Sized>(
        &mut self,
        value: &V,
    ) -> std::result::Result<(), Self::Error> {
        self.0.serialize_value(&Captured(value))
    }
    fn end(self) -> std::result::Result<Value, Self::Error> {
        self.0.end()
    }
}
macro_rules! structure {
    ($trait:ident) => {
        impl<T: ser::$trait<Ok = Value, Error = serde_json::Error>> ser::$trait for Compound<T> {
            type Ok = Value;
            type Error = serde_json::Error;
            fn serialize_field<V: Serialize + ?Sized>(
                &mut self,
                key: &'static str,
                value: &V,
            ) -> std::result::Result<(), Self::Error> {
                self.0.serialize_field(key, &Captured(value))
            }
            fn end(self) -> std::result::Result<Value, Self::Error> {
                self.0.end()
            }
        }
    };
}
structure!(SerializeStruct);
structure!(SerializeStructVariant);
