//! Bounded, source-preserving engineering catalog interchange for spreadsheets.
use crate::{
    CadError, Material, Result,
    material_library::{self, Catalog},
    materials::{MaterialMetadata, Properties},
};
use std::{fs::File, io::Read, path::Path};
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
const HEADER: [&str; 14] = [
    "tcad_material_csv_version",
    "catalog_name",
    "id",
    "name",
    "density_kg_m3",
    "family",
    "grade",
    "condition",
    "description",
    "metadata_json",
    "properties_json",
    "legacy_color_json",
    "legacy_metallic",
    "legacy_roughness",
];
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Material CSV: {}", message.into()))
}
fn rows(text: &str) -> Result<Vec<Vec<String>>> {
    if text.is_empty() || text.len() > MAX_BYTES {
        return Err(invalid("Input must be nonempty UTF-8 no larger than 8 MiB"));
    }
    let mut input = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .chars()
        .peekable();
    let mut result = vec![];
    let mut row = vec![];
    let mut cell = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut started = false;
    while let Some(c) = input.next() {
        if quoted {
            if c == '"' {
                if input.peek() == Some(&'"') {
                    input.next();
                    cell.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                cell.push(c);
            }
        } else {
            match c {
                '"' if !started => {
                    quoted = true;
                    started = true;
                }
                '"' => {
                    return Err(invalid(
                        "A quote inside an unquoted cell must be escaped by quoting the complete cell",
                    ));
                }
                ',' => {
                    row.push(std::mem::take(&mut cell));
                    closed = false;
                    started = false;
                }
                '\r' | '\n' => {
                    if c == '\r' && input.peek() == Some(&'\n') {
                        input.next();
                    }
                    row.push(std::mem::take(&mut cell));
                    result.push(std::mem::take(&mut row));
                    closed = false;
                    started = false;
                }
                _ if closed => return Err(invalid("Unexpected text after a quoted cell")),
                _ => {
                    cell.push(c);
                    started = true;
                }
            }
        }
        if row.len() > HEADER.len() || result.len() > material_library::MAX_MATERIALS + 1 {
            return Err(invalid("Input exceeds the supported row or column count"));
        }
    }
    if quoted {
        return Err(invalid("Unterminated quoted cell"));
    }
    if started || !row.is_empty() {
        row.push(cell);
        result.push(row);
    }
    if result.len() > material_library::MAX_MATERIALS + 1 {
        return Err(invalid("Input exceeds 2000 material rows"));
    }
    Ok(result)
}
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
pub fn export(catalog: &Catalog) -> Result<String> {
    material_library::export_json(catalog)?;
    if catalog.materials.is_empty() {
        return Err(invalid(
            "CSV requires at least one material; use JSON for an empty catalog",
        ));
    }
    let mut output = HEADER.join(",");
    output.push('\n');
    for material in &catalog.materials {
        let metadata = material.metadata.as_ref();
        let family = metadata
            .map(|metadata| serde_json::to_value(metadata.family))
            .transpose()?
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let fields = [
            CONTRACT_VERSION.to_string(),
            catalog.name.clone(),
            material.id.clone(),
            material.name.clone(),
            material.density_kg_m3.to_string(),
            family,
            metadata
                .map(|metadata| metadata.grade.clone())
                .unwrap_or_default(),
            metadata
                .map(|metadata| metadata.condition.clone())
                .unwrap_or_default(),
            metadata
                .map(|metadata| metadata.description.clone())
                .unwrap_or_default(),
            metadata
                .map(serde_json::to_string)
                .transpose()?
                .unwrap_or_default(),
            serde_json::to_string(&material.properties)?,
            serde_json::to_string(&material.color)?,
            material.metallic.to_string(),
            material.roughness.to_string(),
        ];
        output.push_str(
            &fields
                .iter()
                .map(|field| quoted(field))
                .collect::<Vec<_>>()
                .join(","),
        );
        output.push('\n');
        if output.len() > MAX_BYTES {
            return Err(invalid("Export exceeds the 8 MiB CSV bound"));
        }
    }
    Ok(output)
}
pub fn import(text: &str) -> Result<Catalog> {
    let rows = rows(text)?;
    if rows
        .first()
        .is_none_or(|row| row.iter().map(String::as_str).ne(HEADER))
        || rows.len() < 2
    {
        return Err(invalid(
            "Expected the versioned 14-column header and at least one material row",
        ));
    }
    let mut name = None;
    let mut materials = vec![];
    for (index, row) in rows.into_iter().enumerate().skip(1) {
        let result = (|| -> Result<Material> {
            if row.len() != HEADER.len() {
                return Err(invalid("Each material must have exactly 14 columns"));
            }
            if row[0] != CONTRACT_VERSION.to_string() {
                return Err(invalid("Unsupported CSV version"));
            }
            if name.as_ref().is_some_and(|name| name != &row[1]) {
                return Err(invalid("All rows must use the same catalog name"));
            }
            name = Some(row[1].clone());
            let mut metadata = if row[9].is_empty() {
                None
            } else {
                Some(serde_json::from_str::<MaterialMetadata>(&row[9])?)
            };
            if metadata.is_some() || row[5..=8].iter().any(|field| !field.is_empty()) {
                let metadata = metadata.get_or_insert_default();
                metadata.family = if row[5].is_empty() {
                    Default::default()
                } else {
                    serde_json::from_value(serde_json::Value::String(row[5].clone()))?
                };
                metadata.grade = row[6].clone();
                metadata.condition = row[7].clone();
                metadata.description = row[8].clone();
            }
            let material = Material {
                id: row[2].clone(),
                name: row[3].clone(),
                density_kg_m3: row[4]
                    .parse()
                    .map_err(|_| invalid("Density requires a finite decimal number in kg/m³"))?,
                metadata,
                properties: if row[10].is_empty() {
                    Properties::default()
                } else {
                    serde_json::from_str(&row[10])?
                },
                color: serde_json::from_str(&row[11])?,
                metallic: row[12]
                    .parse()
                    .map_err(|_| invalid("Invalid legacy metallic value"))?,
                roughness: row[13]
                    .parse()
                    .map_err(|_| invalid("Invalid legacy roughness value"))?,
            };
            crate::materials::validate(&material)?;
            Ok(material)
        })()
        .map_err(|error| invalid(format!("Row {}: {error}", index + 1)))?;
        materials.push(result);
    }
    let catalog = Catalog::new(
        name.ok_or_else(|| invalid("Missing catalog name"))?,
        materials,
    );
    material_library::export_json(&catalog)?;
    Ok(catalog)
}
/// Bounded exact reads avoid an unbounded allocation if a selected file grows.
pub fn read(path: &Path) -> Result<Catalog> {
    let csv = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"));
    let maximum = if csv {
        MAX_BYTES
    } else {
        material_library::MAX_JSON_BYTES
    };
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(invalid(
            "The catalog file is not regular or exceeds its interchange byte limit",
        ));
    }
    let mut text = String::new();
    file.take(maximum as u64 + 1).read_to_string(&mut text)?;
    if text.len() > maximum {
        return Err(invalid("Catalog file grew beyond its byte limit"));
    }
    if csv {
        import(&text)
    } else {
        material_library::parse_json(&text)
    }
}
