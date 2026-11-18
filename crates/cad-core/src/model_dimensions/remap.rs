//! Carry drawing placement with its source when a part or occurrence is forked.
use crate::{CadError, Id, Result};
use std::collections::BTreeMap;

pub(crate) struct PaperKey {
    part: Id,
    configuration: Option<Id>,
    pub path: Vec<Id>,
    kind: String,
    source: Id,
}
impl PaperKey {
    pub fn decode(key: &str) -> Result<Self> {
        let (part, configuration, path, kind, source) = serde_json::from_str(key)?;
        Ok(Self {
            part,
            configuration,
            path,
            kind,
            source,
        })
    }
    pub fn encode(&self) -> Result<String> {
        Ok(serde_json::to_string(&(
            &self.part,
            &self.configuration,
            &self.path,
            &self.kind,
            &self.source,
        ))?)
    }
    pub fn remap(&mut self, ids: &BTreeMap<Id, Id>) -> Result<()> {
        for id in std::iter::once(&mut self.part)
            .chain(self.configuration.iter_mut())
            .chain(self.path.iter_mut())
        {
            if let Some(replacement) = ids.get(id) {
                *id = replacement.clone();
            }
        }
        match self.kind.as_str() {
            "dimension" => self.source = super::geometry::remap_id(&self.source, ids)?,
            "datum" => {}
            "weld" => {
                if let Some(replacement) = ids.get(&self.source) {
                    self.source.clone_from(replacement);
                }
            }
            _ => return Err(CadError::Invalid("Unknown model drawing item.".into())),
        }
        Ok(())
    }
}

pub(crate) fn paper_item(key: &str, ids: &BTreeMap<Id, Id>) -> Result<String> {
    let mut item = PaperKey::decode(key)?;
    item.remap(ids)?;
    item.encode()
}
