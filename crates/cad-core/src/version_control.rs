use crate::{CadError, Document, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::Command,
};
pub trait VersionControl {
    fn command(&self, root: &Path, args: &[&str]) -> Result<String>;
}
pub struct Git;
impl VersionControl for Git {
    fn command(&self, root: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into())
        } else {
            Err(CadError::Invalid(
                String::from_utf8_lossy(&output.stderr).into(),
            ))
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticChange {
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
fn entities(value: &Value) -> Option<BTreeMap<&str, &Value>> {
    let Value::Array(array) = value else {
        return None;
    };
    let mut map = BTreeMap::new();
    for item in array {
        let id = item.get("id")?.as_str()?;
        if map.insert(id, item).is_some() {
            return None;
        }
    }
    Some(map)
}
pub fn diff(before: &Document, after: &Document) -> Result<Vec<SemanticChange>> {
    fn walk(path: String, a: Option<&Value>, b: Option<&Value>, out: &mut Vec<SemanticChange>) {
        if a == b {
            return;
        }
        if let (Some(a), Some(b)) = (a, b) {
            if let (Value::Object(a), Value::Object(b)) = (a, b) {
                let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
                for k in keys {
                    walk(format!("{path}/{k}"), a.get(k), b.get(k), out);
                }
                return;
            }
            if let (Some(a), Some(b)) = (entities(a), entities(b)) {
                let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
                for k in keys {
                    walk(
                        format!("{path}/{k}"),
                        a.get(k).copied(),
                        b.get(k).copied(),
                        out,
                    );
                }
                return;
            }
        }
        out.push(SemanticChange {
            path,
            before: a.cloned(),
            after: b.cloned(),
        });
    }
    let mut out = vec![];
    walk(
        String::new(),
        Some(&serde_json::to_value(before)?),
        Some(&serde_json::to_value(after)?),
        &mut out,
    );
    Ok(out)
}
pub fn merge(base: &Document, ours: &Document, theirs: &Document) -> Result<Document> {
    fn combine(path: &str, b: &Value, o: &Value, t: &Value) -> Result<Value> {
        if o == t || t == b {
            return Ok(o.clone());
        }
        if o == b {
            return Ok(t.clone());
        }
        if path == "/revision" {
            return Ok(serde_json::json!(
                o.as_u64().unwrap_or(0).max(t.as_u64().unwrap_or(0))
            ));
        }
        if let (Value::Object(b), Value::Object(o), Value::Object(t)) = (b, o, t) {
            let keys: BTreeSet<_> = b.keys().chain(o.keys()).chain(t.keys()).collect();
            let mut result = serde_json::Map::new();
            for k in keys {
                let value = combine(
                    &format!("{path}/{k}"),
                    b.get(k).unwrap_or(&Value::Null),
                    o.get(k).unwrap_or(&Value::Null),
                    t.get(k).unwrap_or(&Value::Null),
                )?;
                if !value.is_null() {
                    result.insert(k.clone(), value);
                }
            }
            return Ok(Value::Object(result));
        }
        if let (Some(bm), Some(om), Some(tm)) = (entities(b), entities(o), entities(t)) {
            let keys: BTreeSet<_> = bm.keys().chain(om.keys()).chain(tm.keys()).collect();
            let mut merged = BTreeMap::new();
            for key in keys {
                let value = combine(
                    &format!("{path}/{key}"),
                    bm.get(key).copied().unwrap_or(&Value::Null),
                    om.get(key).copied().unwrap_or(&Value::Null),
                    tm.get(key).copied().unwrap_or(&Value::Null),
                )?;
                if !value.is_null() {
                    merged.insert(key.to_string(), value);
                }
            }
            let mut order = vec![];
            for item in o
                .as_array()
                .into_iter()
                .flatten()
                .chain(t.as_array().into_iter().flatten())
            {
                if let Some(id) = item.get("id").and_then(Value::as_str)
                    && !order.contains(&id.to_string())
                {
                    order.push(id.into());
                }
            }
            return Ok(Value::Array(
                order
                    .into_iter()
                    .filter_map(|id| merged.remove(&id))
                    .collect(),
            ));
        }
        Err(CadError::Invalid(format!(
            "Semantic merge conflict at {path}"
        )))
    }
    if base.id != ours.id || base.id != theirs.id {
        return Err(CadError::Invalid(
            "Merge inputs must share one document identity".into(),
        ));
    }
    let value = combine(
        "",
        &serde_json::to_value(base)?,
        &serde_json::to_value(ours)?,
        &serde_json::to_value(theirs)?,
    )?;
    let doc: Document = serde_json::from_value(value)?;
    doc.validate()?;
    Ok(doc)
}
/// Read a complete native document at a Git revision without changing the checkout.
pub fn snapshot(root: &Path, revision: &str) -> Result<Document> {
    let revision = Git.command(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{revision}^{{commit}}"),
        ],
    )?;
    let revision = revision.trim();
    let manifest: Value =
        serde_json::from_str(&Git.command(root, &["show", &format!("{revision}:project.json")])?)?;
    let mut doc: Document = serde_json::from_value(
        manifest
            .get("document")
            .cloned()
            .ok_or_else(|| CadError::Invalid("Revision has no native document".into()))?,
    )?;
    for (kind, part) in [("parts", true), ("assemblies", false)] {
        let map = manifest
            .get(kind)
            .and_then(Value::as_object)
            .ok_or_else(|| CadError::Invalid("Invalid revision manifest".into()))?;
        for (id, path) in map {
            let path = path
                .as_str()
                .ok_or_else(|| CadError::Invalid("Invalid revision path".into()))?;
            crate::persistence::safe_join(root, path)?;
            let source = Git.command(root, &["show", &format!("{revision}:{path}")])?;
            if part {
                doc.parts.insert(id.clone(), serde_json::from_str(&source)?);
            } else {
                doc.assemblies
                    .insert(id.clone(), serde_json::from_str(&source)?);
            }
        }
    }
    doc.validate()?;
    Ok(doc)
}
/// Materialize only pinned resource bytes in a disposable comparison root; never check out a revision.
pub fn snapshot_resources(root: &Path, revision: &str, destination: &Path) -> Result<Document> {
    use sha2::{Digest, Sha256};
    let revision = Git.command(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{revision}^{{commit}}"),
        ],
    )?;
    let revision = revision.trim();
    let document = snapshot(root, revision)?;
    for resource in document.resources.values() {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["show", &format!("{revision}:{}", resource.path)])
            .output()?;
        if !output.status.success() {
            return Err(CadError::Invalid(format!(
                "Revision lacks pinned resource {}",
                resource.path
            )));
        }
        if format!("{:x}", Sha256::digest(&output.stdout)) != resource.sha256 {
            return Err(CadError::Invalid(
                "Revision resource bytes do not match their recorded SHA-256".into(),
            ));
        }
        crate::persistence::atomic_write(
            &crate::persistence::safe_join(destination, &resource.path)?,
            &output.stdout,
        )?;
    }
    Ok(document)
}
