use crate::{CadError, Document, Id, Result, protocol::Evaluation};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BomRow {
    pub definition: Id,
    pub part_number: String,
    pub description: String,
    pub material: String,
    pub quantity: usize,
    pub configuration: Option<Id>,
}
pub fn bom(doc: &Document) -> Result<Vec<BomRow>> {
    Ok(crate::bom::rows(doc, &crate::bom::Options::default())?
        .into_iter()
        .map(|row| BomRow {
            definition: row.definition,
            part_number: row.part_number,
            description: row.description,
            material: row.material,
            quantity: row.quantity,
            configuration: row.configuration,
        })
        .collect())
}
/// Configurable native BOM contract. The legacy `bom` API remains parts-only.
pub fn bom_with_options(
    doc: &Document,
    options: &crate::bom::Options,
) -> Result<Vec<crate::bom::Row>> {
    crate::bom::rows(doc, options)
}
pub fn csv_with_options(doc: &Document, options: &crate::bom::Options) -> Result<String> {
    crate::bom::csv(doc, options)
}

pub fn csv(doc: &Document) -> Result<String> {
    fn escaped(s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
    let mut out = "Part number,Description,Material,Quantity,Configuration\n".to_string();
    for row in bom(doc)? {
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            escaped(&row.part_number),
            escaped(&row.description),
            escaped(&row.material),
            row.quantity,
            escaped(row.configuration.as_deref().unwrap_or(""))
        ));
    }
    Ok(out)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bend {
    pub angle_deg: f64,
    pub inside_radius_mm: f64,
    pub thickness_mm: f64,
    pub k_factor: f64,
}
impl Bend {
    pub fn allowance(&self) -> Result<f64> {
        if ![
            self.angle_deg,
            self.inside_radius_mm,
            self.thickness_mm,
            self.k_factor,
        ]
        .iter()
        .all(|v| v.is_finite())
            || self.inside_radius_mm < 0.
            || self.thickness_mm <= 0.
            || !(0.0..=1.0).contains(&self.k_factor)
        {
            return Err(CadError::Invalid("Invalid bend parameters".into()));
        }
        Ok(self.angle_deg.abs().to_radians()
            * (self.inside_radius_mm + self.k_factor * self.thickness_mm))
    }
}
pub fn flat_length(straight_sections: &[f64], bends: &[Bend]) -> Result<f64> {
    let bend: f64 = bends
        .iter()
        .map(Bend::allowance)
        .collect::<Result<Vec<_>>>()?
        .iter()
        .sum();
    if straight_sections.iter().any(|v| !v.is_finite() || *v < 0.) {
        return Err(CadError::Invalid("Invalid straight sheet length".into()));
    }
    Ok(straight_sections.iter().sum::<f64>() + bend)
}
pub fn dxf_profiles(doc: &Document) -> Result<String> {
    fn line(out: &mut String, layer: &str, a: [f64; 2], b: [f64; 2]) {
        out.push_str(&format!(
            "0\nLINE\n8\n{}\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n",
            layer.replace(['\n', '\r'], " "),
            a[0],
            a[1],
            b[0],
            b[1]
        ));
    }
    let mut out =
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1027\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n"
            .to_string();
    for part in doc.parts.values() {
        for feature in &part.features {
            let mut polygon = None;
            match &feature.operation {
                crate::Operation::Profile {
                    profile: crate::Profile::Polygon { points },
                    ..
                } => polygon = Some(points.clone()),
                crate::Operation::Profile {
                    profile: crate::Profile::Sketch { sketch },
                    ..
                } => {
                    use crate::solver::ConstraintSolver;
                    let mut sketch = part
                        .sketches
                        .get(sketch)
                        .ok_or_else(|| CadError::Invalid("Missing DXF sketch".into()))?
                        .clone();
                    crate::solver::NativeSolver.solve_sketch(doc, &part.id, &mut sketch)?;
                    for circle in sketch.circles.values() {
                        let center = sketch.points[&circle.center];
                        let radius = crate::expression::value(
                            doc,
                            &part.id,
                            &circle.radius,
                            crate::Dimension::Length,
                        )?;
                        out.push_str(&format!(
                            "0\nCIRCLE\n8\n{}\n10\n{}\n20\n{}\n30\n0\n40\n{}\n",
                            part.name.replace(['\n', '\r'], " "),
                            center[0],
                            center[1],
                            radius
                        ));
                    }
                    for member in sketch.members.values().filter(|m| !m.construction) {
                        use crate::SketchCurve;
                        let layer = feature.id.replace(['\n', '\r'], " ");
                        match &member.curve {
                            SketchCurve::Line { start, end } => {
                                line(&mut out, &layer, sketch.points[start], sketch.points[end])
                            }
                            SketchCurve::Circle { center, radius } => {
                                let c = sketch.points[center];
                                let r = crate::expression::value(
                                    doc,
                                    &part.id,
                                    radius,
                                    crate::Dimension::Length,
                                )?;
                                out.push_str(&format!(
                                    "0\nCIRCLE\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n40\n{r}\n",
                                    c[0], c[1]
                                ));
                            }
                            SketchCurve::Arc {
                                start,
                                through,
                                end,
                            } => {
                                let (c, r, a, sweep) = crate::sketch_geometry::arc_frame(
                                    sketch.points[start],
                                    sketch.points[through],
                                    sketch.points[end],
                                )?;
                                let (a, b) = if sweep > 0. {
                                    (a, a + sweep)
                                } else {
                                    (a + sweep, a)
                                };
                                out.push_str(&format!("0\nARC\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n40\n{r}\n50\n{}\n51\n{}\n",c[0],c[1],a.to_degrees().rem_euclid(360.),b.to_degrees().rem_euclid(360.)));
                            }
                            SketchCurve::Ellipse {
                                center,
                                major,
                                minor,
                                angle,
                            } => {
                                let c = sketch.points[center];
                                let mut a = crate::expression::value(
                                    doc,
                                    &part.id,
                                    major,
                                    crate::Dimension::Length,
                                )?;
                                let mut b = crate::expression::value(
                                    doc,
                                    &part.id,
                                    minor,
                                    crate::Dimension::Length,
                                )?;
                                let mut rotation = crate::expression::value(
                                    doc,
                                    &part.id,
                                    angle,
                                    crate::Dimension::Angle,
                                )?;
                                if b > a {
                                    std::mem::swap(&mut a, &mut b);
                                    rotation += std::f64::consts::FRAC_PI_2;
                                }
                                out.push_str(&format!("0\nELLIPSE\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n40\n{}\n41\n0\n42\n{}\n",c[0],c[1],a*rotation.cos(),a*rotation.sin(),b/a,std::f64::consts::TAU));
                            }
                            SketchCurve::Nurbs {
                                points,
                                weights,
                                knots,
                                degree,
                            } => {
                                out.push_str(&format!("0\nSPLINE\n8\n{layer}\n70\n4\n71\n{degree}\n72\n{}\n73\n{}\n74\n0\n",knots.len(),points.len()));
                                for knot in knots {
                                    out.push_str(&format!("40\n{knot}\n"));
                                }
                                for weight in weights {
                                    out.push_str(&format!("41\n{weight}\n"));
                                }
                                for id in points {
                                    let p = sketch.points[id];
                                    out.push_str(&format!("10\n{}\n20\n{}\n30\n0\n", p[0], p[1]));
                                }
                            }
                            SketchCurve::Bezier { points } => {
                                let count = points.len();
                                out.push_str(&format!("0\nSPLINE\n8\n{layer}\n70\n8\n71\n{}\n72\n{}\n73\n{count}\n74\n0\n",count-1,2*count));
                                for value in [0, 1] {
                                    for _ in 0..count {
                                        out.push_str(&format!("40\n{value}\n"));
                                    }
                                }
                                for id in points {
                                    let p = sketch.points[id];
                                    out.push_str(&format!("10\n{}\n20\n{}\n30\n0\n", p[0], p[1]));
                                }
                            }
                        }
                    }
                    polygon = Some(
                        sketch
                            .profile_order
                            .iter()
                            .map(|id| sketch.points[id])
                            .collect(),
                    );
                }
                crate::Operation::Profile {
                    profile: crate::Profile::Circle { radius, center },
                    ..
                } => {
                    let r =
                        crate::expression::value(doc, &part.id, radius, crate::Dimension::Length)?;
                    out.push_str(&format!(
                        "0\nCIRCLE\n8\n{}\n10\n{}\n20\n{}\n30\n0\n40\n{}\n",
                        part.id, center[0], center[1], r
                    ));
                }
                _ => {}
            }
            if let Some(points) = polygon {
                for i in 0..points.len() {
                    line(
                        &mut out,
                        &feature.id,
                        points[i],
                        points[(i + 1) % points.len()],
                    );
                }
            }
        }
    }
    let mut offset = 0.;
    for (id, length, width, bends) in flat_patterns(doc)? {
        let points = [
            [offset, 0.],
            [offset + length, 0.],
            [offset + length, width],
            [offset, width],
        ];
        for i in 0..4 {
            line(
                &mut out,
                &format!("flat-{id}"),
                points[i],
                points[(i + 1) % 4],
            );
        }
        for bend in bends {
            line(
                &mut out,
                &format!("bend-{id}"),
                [offset + bend, 0.],
                [offset + bend, width],
            );
        }
        offset += length + 20.;
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    Ok(out)
}

pub fn stl(evaluation: &Evaluation, path: &Path) -> Result<()> {
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "solid tcad")?;
    for instance in evaluation.instances.iter().chain(
        evaluation
            .assembly_welds
            .values()
            .map(|weld| &weld.placement),
    ) {
        if let Some(mesh) = evaluation.meshes.get(&instance.mesh_key) {
            let transform = |p: [f64; 3]| -> nalgebra::Vector3<f64> {
                nalgebra::Vector3::from(std::array::from_fn(|i| {
                    instance.matrix[i][0] * p[0]
                        + instance.matrix[i][1] * p[1]
                        + instance.matrix[i][2] * p[2]
                        + instance.matrix[i][3]
                }))
            };
            for triangle in mesh.indices.as_chunks::<3>().0 {
                let p = triangle
                    .iter()
                    .map(|i| transform(mesh.positions[*i as usize]))
                    .collect::<Vec<_>>();
                let normal = (p[1] - p[0])
                    .cross(&(p[2] - p[0]))
                    .try_normalize(1e-12)
                    .unwrap_or_default();
                writeln!(
                    file,
                    "facet normal {} {} {}\n outer loop",
                    normal.x, normal.y, normal.z
                )?;
                for p in p {
                    writeln!(file, " vertex {} {} {}", p.x, p.y, p.z)?;
                }
                writeln!(file, " endloop\nendfacet")?;
            }
        }
    }
    writeln!(file, "endsolid tcad")?;
    Ok(())
}
pub fn drawing_pdf(doc: &Document, e: &Evaluation, path: &Path) -> Result<()> {
    let factor = 72. / 25.4;
    let mut pages = vec![];
    for sheet in crate::drawing::sheets(doc, e)? {
        let mut content = String::from("0.45 w\n");
        for primitive in sheet.primitives {
            match primitive {
                crate::drawing::Primitive::Polyline { points, dashed } => {
                    content.push_str(if dashed {
                        "[11.34 2.83 2.83 2.83] 0 d\n"
                    } else {
                        "[] 0 d\n"
                    });
                    for (i, p) in points.iter().enumerate() {
                        content.push_str(&format!(
                            "{:.6} {:.6} {}\n",
                            p[0] * factor,
                            p[1] * factor,
                            if i == 0 { "m" } else { "l" }
                        ));
                    }
                    content.push_str("S\n");
                }
                crate::drawing::Primitive::Text { at, size, text } => {
                    content.push_str(&format!(
                        "BT /F1 {} Tf {} {} Td ({}) Tj ET\n",
                        size * factor,
                        at[0] * factor,
                        at[1] * factor,
                        pdf_escape(&text)?
                    ));
                }
                crate::drawing::Primitive::Spline { curve, dashed } => {
                    content.push_str(if dashed {
                        "[11.34 2.83 2.83 2.83] 0 d\n"
                    } else {
                        "[] 0 d\n"
                    });
                    for (i, p) in crate::drawing::display_curve(&curve)?.iter().enumerate() {
                        content.push_str(&format!(
                            "{:.6} {:.6} {}\n",
                            p[0] * factor,
                            p[1] * factor,
                            if i == 0 { "m" } else { "l" }
                        ));
                    }
                    content.push_str("S\n");
                }
                crate::drawing::Primitive::Circle { center, radius } => {
                    let x = center[0] * factor;
                    let y = center[1] * factor;
                    let r = radius * factor;
                    let k = r * 0.5522847498307936;
                    content.push_str(&format!("[] 0 d\n{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c S\n",x+r,y,x+r,y+k,x+k,y+r,x,y+r,x-k,y+r,x-r,y+k,x-r,y,x-r,y-k,x-k,y-r,x,y-r,x+k,y-r,x+r,y-k,x+r,y));
                }
            }
        }
        pages.push((
            sheet.paper_mm[0] * factor,
            sheet.paper_mm[1] * factor,
            content,
        ));
    }
    let count = pages.len();
    let kids = (0..count)
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
    ];
    for (index, (w, h, content)) in pages.into_iter().enumerate() {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",5+index*2));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ));
    }
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend(format!("{} 0 obj\n{}\nendobj\n", i + 1, object).bytes());
    }
    let xref = data.len();
    data.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).bytes());
    for offset in offsets.into_iter().skip(1) {
        data.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    data.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    crate::persistence::atomic_write(path, &data)
}

fn pdf_escape(s: &str) -> Result<String> {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\n' | '\r' => out.push(' '),
            c if c.is_ascii_graphic() || c == ' ' => out.push(c),
            c if (160..=255).contains(&(c as u32)) => out.push_str(&format!("\\{:03o}", c as u32)),
            _ => {
                return Err(CadError::Unsupported(format!(
                    "PDF annotation font cannot encode {c:?}; use a supported Latin annotation character"
                )));
            }
        }
    }
    Ok(out)
}

pub const SOURCE_LABEL_VERSION: u32 = 1;
/// Disposable human-readable source metadata. Stable fabrication grouping and
/// reference resolution continue to use the separate authored NodeRef.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceLabel {
    pub version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_name: Option<String>,
}
impl SourceLabel {
    pub fn display(&self) -> String {
        match &self.library_name {
            Some(library) if !library.is_empty() => format!("{library} · {}", self.name),
            _ => self.name.clone(),
        }
    }
}
/// Resolve presentation names from current definitions, including imported
/// library profile names. No geometry, grouping key, or source ID changes.
pub fn section_label(doc: &Document, source: &crate::NodeRef) -> Option<SourceLabel> {
    let part = doc.parts.get(&source.definition)?;
    let name = part
        .features
        .iter()
        .find(|feature| feature.id == source.node)
        .map(|feature| feature.name.as_str())
        .or_else(|| {
            part.sketches
                .get(&source.node)
                .map(|sketch| sketch.name.as_str())
        })?;
    let library_name = doc
        .libraries
        .values()
        .find(|link| {
            link.definition == source.definition
                || link.pinned_definitions.contains_key(&source.definition)
        })
        .map(|link| link.name.clone());
    Some(SourceLabel {
        version: SOURCE_LABEL_VERSION,
        name: format!("{} / {name}", part.name),
        library_name,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CutListRow {
    pub definition: Id,
    pub feature: Id,
    pub description: String,
    pub material: String,
    pub length_mm: f64,
    pub quantity: usize,
    #[serde(default)]
    pub configuration: Option<Id>,
    #[serde(default)]
    pub member_reference: Option<crate::weldment::MemberRef>,
    #[serde(default)]
    pub section: Option<crate::NodeRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_label: Option<SourceLabel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_size: Option<crate::section_size::SectionSize>,
    /// Saved catalog designation/provenance; native solved section_size remains
    /// the only dimensional measurement supplied here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_profile: Option<crate::weldment::ProfileLabel>,
    #[serde(default)]
    pub start_cut_angle_rad: Option<f64>,
    #[serde(default)]
    pub end_cut_angle_rad: Option<f64>,
    #[serde(default)]
    pub bend_radius_mm: Option<f64>,
    #[serde(default)]
    pub sweep_angle_rad: Option<f64>,
    #[serde(default)]
    pub accessory: Option<crate::weldment::AccessoryKind>,
    #[serde(default)]
    pub blank_size_mm: Option<[f64; 2]>,
    #[serde(default)]
    pub thickness_mm: Option<f64>,
    #[serde(default)]
    pub blank_bounds_exact: Option<bool>,
}
pub fn section_display(row: &CutListRow, units: crate::drawing::Units, precision: usize) -> String {
    let name = row
        .library_profile
        .as_ref()
        .and_then(crate::weldment::ProfileLabel::display)
        .or_else(|| {
            row.section_label
                .as_ref()
                .filter(|label| label.version == SOURCE_LABEL_VERSION)
                .map(SourceLabel::display)
        })
        .unwrap_or_else(|| {
            if row.section.is_some() {
                "Section unavailable".into()
            } else {
                String::new()
            }
        });
    match row
        .section_size
        .as_ref()
        .and_then(|size| size.display(units, precision))
    {
        Some(size) => format!("{name} · {size}"),
        None => name,
    }
}
fn evaluated_section_size(
    doc: &Document,
    evaluation: &Evaluation,
    source: &crate::NodeRef,
    owner: &str,
    configuration: Option<&str>,
) -> Option<crate::section_size::SectionSize> {
    let configuration = if source.definition == owner {
        configuration
    } else {
        doc.active_configurations
            .get(&source.definition)
            .map(String::as_str)
    };
    evaluation
        .sketches
        .get(&format!(
            "{}::{}",
            crate::configuration::key(&source.definition, configuration),
            source.node
        ))
        .and_then(crate::section_size::from_sketch)
}
pub fn cut_list(doc: &Document) -> Result<Vec<CutListRow>> {
    let grouped = crate::bom::rows(doc, &crate::bom::Options::default())?
        .into_iter()
        .map(|row| ((row.definition, row.configuration), row.quantity))
        .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::new();
    for ((definition, configuration), quantity) in grouped {
        let p = crate::configuration::effective_part(doc, &definition, configuration.as_deref())?;
        for f in p.features.iter().filter(|f| !f.suppressed) {
            if let crate::Operation::StructuralMember { start, end, .. } = &f.operation {
                rows.push(CutListRow {
                    definition: p.id.clone(),
                    feature: f.id.clone(),
                    description: if let Some(configuration) = &configuration {
                        format!(
                            "{} · {}",
                            f.name,
                            crate::configuration::resolve(doc, &definition, configuration)?.name
                        )
                    } else {
                        f.name.clone()
                    },
                    material: p.material.clone(),
                    length_mm: (0..3)
                        .map(|i| (end[i] - start[i]).powi(2))
                        .sum::<f64>()
                        .sqrt(),
                    quantity,
                    configuration: configuration.clone(),
                    member_reference: None,
                    section: None,
                    section_label: None,
                    section_size: None,
                    library_profile: None,
                    start_cut_angle_rad: None,
                    end_cut_angle_rad: None,
                    bend_radius_mm: None,
                    sweep_angle_rad: None,
                    accessory: None,
                    blank_size_mm: None,
                    thickness_mm: None,
                    blank_bounds_exact: None,
                });
            }
        }
    }
    Ok(rows)
}
pub fn cut_list_csv(doc: &Document) -> Result<String> {
    let mut s = "Definition,Member,Material,Reference-path length (mm),Quantity\n".to_string();
    for r in cut_list(doc)? {
        s.push_str(&format!(
            "{},\"{}\",\"{}\",{:.6},{}\n",
            r.definition,
            r.description.replace('"', "\"\""),
            r.material.replace('"', "\"\""),
            r.length_mm,
            r.quantity
        ));
    }
    Ok(s)
}

/// Manufacturing rows from the resolved end planes of the evaluated native members.
/// Length is measured on the path centerline, not the longest exterior stock edge.
pub fn cut_list_evaluated(doc: &Document, evaluation: &Evaluation) -> Result<Vec<CutListRow>> {
    let mut rows = cut_list(doc)?;
    append_native_cut_list(doc, evaluation, None, &[], None, &mut rows)?;
    Ok(rows)
}
/// Source-linked stock grouping and customizable manufacturing columns.
pub fn cut_list_with_options(
    doc: &Document,
    evaluation: &Evaluation,
    options: &crate::cut_list::Options,
) -> Result<Vec<crate::cut_list::Group>> {
    crate::cut_list::rows(doc, evaluation, options)
}
pub fn cut_list_csv_with_options(
    doc: &Document,
    evaluation: &Evaluation,
    options: &crate::cut_list::Options,
    units: crate::drawing::Units,
    precision: usize,
) -> Result<String> {
    crate::cut_list::csv(doc, evaluation, options, units, precision)
}
/// Native drawing rows admitted only after occurrence/configuration/output filtering.
/// The row limit is an error boundary, never a truncation into a complete table.
pub fn native_cut_list_for_drawing(
    doc: &Document,
    evaluation: &Evaluation,
    represented: &BTreeSet<(Id, Option<Id>)>,
    outputs: &[crate::NodeRef],
    limit: usize,
) -> Result<Vec<CutListRow>> {
    let mut rows = Vec::new();
    append_native_cut_list(
        doc,
        evaluation,
        Some(represented),
        outputs,
        Some(limit),
        &mut rows,
    )?;
    Ok(rows)
}
fn append_native_cut_list(
    doc: &Document,
    evaluation: &Evaluation,
    represented: Option<&BTreeSet<(Id, Option<Id>)>>,
    outputs: &[crate::NodeRef],
    limit: Option<usize>,
    rows: &mut Vec<CutListRow>,
) -> Result<()> {
    let outputs = outputs.iter().cloned().collect::<BTreeSet<_>>();
    let mut quantities: BTreeMap<(Id, Option<Id>), usize> = BTreeMap::new();
    for instance in doc.placed_parts()? {
        if instance.bom_excluded
            || represented.is_some_and(|scope| {
                !scope.contains(&(instance.definition.clone(), instance.configuration.clone()))
            })
        {
            continue;
        }
        let config = instance
            .configuration
            .as_ref()
            .map(|id| crate::configuration::resolve(doc, &instance.definition, id))
            .transpose()?;
        if config
            .as_ref()
            .and_then(|config| config.overrides.bom.exclude)
            .unwrap_or(false)
        {
            continue;
        }
        *quantities
            .entry((instance.definition, instance.configuration))
            .or_default() += 1;
    }
    for ((definition, configuration), quantity) in quantities {
        let part =
            crate::configuration::effective_part(doc, &definition, configuration.as_deref())?;
        let key = crate::configuration::key(&definition, configuration.as_deref());
        let suffix = configuration
            .as_ref()
            .map(|id| {
                crate::configuration::resolve(doc, &definition, id)
                    .map(|config| format!(" · {}", config.name))
            })
            .transpose()?
            .unwrap_or_default();
        for feature in part.features.iter().filter(|feature| {
            !feature.suppressed
                && part.outputs.contains(&feature.id)
                && matches!(feature.operation, crate::Operation::Weldment { .. })
                && (outputs.is_empty() || outputs.contains(&part.reference(&feature.id)))
        }) {
            let evaluated = evaluation
                .weldments
                .get(&format!("{key}::{}", feature.id))
                .ok_or_else(|| {
                    CadError::Invalid(format!(
                        "Native cut list requires the current evaluated weldment {} / {}",
                        part.name, feature.name
                    ))
                })?;
            if evaluated.version != crate::weldment::CONTRACT_VERSION
                || evaluated.source != part.reference(&feature.id)
            {
                return Err(CadError::Invalid(
                    "Native cut list requires current fabrication metadata".into(),
                ));
            }
            for (index, member) in evaluated.members.iter().enumerate() {
                if limit.is_some_and(|limit| rows.len() >= limit) {
                    return Err(CadError::Invalid(
                        "Native drawing cut list exceeds its bounded active row limit".into(),
                    ));
                }
                if !member.centerline_length_mm.is_finite()
                    || member.centerline_length_mm <= 0.
                    || !member.start_cut_angle_rad.is_finite()
                    || !member.end_cut_angle_rad.is_finite()
                {
                    return Err(CadError::Invalid(
                        "Native cut-list member has invalid measured end geometry".into(),
                    ));
                }
                let group = part
                    .features
                    .iter()
                    .find(|source| source.id == member.reference.group.node)
                    .map(|source| source.name.as_str())
                    .unwrap_or("Member group");
                rows.push(CutListRow {
                    definition: definition.clone(),
                    feature: feature.id.clone(),
                    description: if let (Some(radius), Some(angle)) =
                        (member.bend_radius_mm, member.sweep_angle_rad)
                    {
                        format!(
                            "{group} · Member {} · Bend R{radius:.3} mm / {:.3} deg{suffix}",
                            index + 1,
                            angle.to_degrees()
                        )
                    } else {
                        format!("{group} · Member {}{suffix}", index + 1)
                    },
                    material: doc
                        .materials
                        .get(&part.material)
                        .map(|material| material.name.clone())
                        .unwrap_or_else(|| part.material.clone()),
                    length_mm: member.centerline_length_mm,
                    quantity,
                    configuration: configuration.clone(),
                    member_reference: Some(member.reference.clone()),
                    section: Some(member.section.clone()),
                    section_label: section_label(doc, &member.section),
                    section_size: evaluated_section_size(
                        doc,
                        evaluation,
                        &member.section,
                        &definition,
                        configuration.as_deref(),
                    ),
                    library_profile: member.library_profile.clone(),
                    start_cut_angle_rad: Some(member.start_cut_angle_rad),
                    end_cut_angle_rad: Some(member.end_cut_angle_rad),
                    bend_radius_mm: member.bend_radius_mm,
                    sweep_angle_rad: member.sweep_angle_rad,
                    accessory: None,
                    blank_size_mm: None,
                    thickness_mm: None,
                    blank_bounds_exact: None,
                });
            }
            for accessory in &evaluated.accessories {
                if limit.is_some_and(|limit| rows.len() >= limit) {
                    return Err(CadError::Invalid(
                        "Native drawing cut list exceeds its bounded active row limit".into(),
                    ));
                }
                if !accessory.thickness_mm.is_finite()
                    || accessory.thickness_mm <= 0.
                    || accessory
                        .blank_size_mm
                        .iter()
                        .any(|v| !v.is_finite() || *v <= 0.)
                {
                    return Err(CadError::Invalid(
                        "Native cut-list accessory has invalid stock dimensions".into(),
                    ));
                }
                let name = part
                    .features
                    .iter()
                    .find(|source| source.id == accessory.reference.group.node)
                    .map(|source| source.name.as_str())
                    .unwrap_or("Accessory");
                let bounds = if accessory.blank_bounds_exact {
                    "blank"
                } else {
                    "control-hull stock bounds"
                };
                rows.push(CutListRow {
                    definition: definition.clone(),
                    feature: feature.id.clone(),
                    description: format!(
                        "{name} · {bounds} {:.3} × {:.3} × {:.3} mm{suffix}",
                        accessory.blank_size_mm[0],
                        accessory.blank_size_mm[1],
                        accessory.thickness_mm
                    ),
                    material: doc
                        .materials
                        .get(&part.material)
                        .map(|material| material.name.clone())
                        .unwrap_or_else(|| part.material.clone()),
                    length_mm: accessory.blank_size_mm[0].max(accessory.blank_size_mm[1]),
                    quantity,
                    configuration: configuration.clone(),
                    member_reference: Some(accessory.reference.clone()),
                    section: Some(accessory.profile.clone()),
                    section_label: section_label(doc, &accessory.profile),
                    section_size: evaluated_section_size(
                        doc,
                        evaluation,
                        &accessory.profile,
                        &definition,
                        configuration.as_deref(),
                    ),
                    library_profile: None,
                    start_cut_angle_rad: Some(0.),
                    end_cut_angle_rad: Some(0.),
                    bend_radius_mm: None,
                    sweep_angle_rad: None,
                    accessory: Some(accessory.kind),
                    blank_size_mm: Some(accessory.blank_size_mm),
                    thickness_mm: Some(accessory.thickness_mm),
                    blank_bounds_exact: Some(accessory.blank_bounds_exact),
                });
            }
        }
    }
    Ok(())
}

pub fn cut_list_csv_evaluated(doc: &Document, evaluation: &Evaluation) -> Result<String> {
    let escaped = |text: &str| format!("\"{}\"", text.replace('"', "\"\""));
    let mut out = "Definition,Member,Material,Section,Centerline or stock length mm,Start cut angle deg,Finish cut angle deg,Quantity,Configuration,Member source,Bend radius mm,Bend sweep deg,Accessory,Stock width mm,Stock height mm,Thickness mm,Stock bounds exact,Section source,Profile catalog,Profile revision,Profile source SHA256,Nominal profile size\n".to_string();
    for row in cut_list_evaluated(doc, evaluation)? {
        let source = row
            .member_reference
            .as_ref()
            .map(|member| {
                format!(
                    "{}::{}::{}",
                    member.group.definition, member.group.node, member.member
                )
            })
            .unwrap_or_else(|| row.feature.clone());
        let section_source = row
            .section
            .as_ref()
            .map(|section| format!("{}::{}", section.definition, section.node))
            .unwrap_or_default();
        let section = section_display(&row, crate::drawing::Units::Millimeters, 3);
        let angle = |value: Option<f64>| {
            value
                .map(|angle| format!("{:.6}", angle.to_degrees()))
                .unwrap_or_default()
        };
        out.push_str(&format!(
            "{},{},{},{},{:.6},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            escaped(&row.definition),
            escaped(&row.description),
            escaped(&row.material),
            escaped(&section),
            row.length_mm,
            angle(row.start_cut_angle_rad),
            angle(row.end_cut_angle_rad),
            row.quantity,
            escaped(row.configuration.as_deref().unwrap_or_default()),
            escaped(&source),
            row.bend_radius_mm
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            angle(row.sweep_angle_rad),
            row.accessory
                .map(|kind| format!("{kind:?}"))
                .unwrap_or_default(),
            row.blank_size_mm
                .map(|size| format!("{:.6}", size[0]))
                .unwrap_or_default(),
            row.blank_size_mm
                .map(|size| format!("{:.6}", size[1]))
                .unwrap_or_default(),
            row.thickness_mm
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default(),
            row.blank_bounds_exact
                .map(|v| v.to_string())
                .unwrap_or_default(),
            escaped(&section_source),
            escaped(
                row.library_profile
                    .as_ref()
                    .map(|profile| profile.catalog_name.as_str())
                    .unwrap_or_default()
            ),
            escaped(
                row.library_profile
                    .as_ref()
                    .map(|profile| profile.profile_revision.as_str())
                    .unwrap_or_default()
            ),
            escaped(
                row.library_profile
                    .as_ref()
                    .map(|profile| profile.source_sha256.as_str())
                    .unwrap_or_default()
            ),
            escaped(
                row.library_profile
                    .as_ref()
                    .map(|profile| profile.nominal_size.as_str())
                    .unwrap_or_default()
            ),
        ));
    }
    Ok(out)
}
pub type FlatPattern = (Id, f64, f64, Vec<f64>);
pub fn flat_patterns(doc: &Document) -> Result<Vec<FlatPattern>> {
    let mut patterns = vec![];
    for p in doc.parts.values() {
        for f in &p.features {
            if let crate::Operation::SheetMetal {
                width,
                thickness,
                sections,
                bends,
            } = &f.operation
            {
                let val = |v: &crate::Expr| {
                    crate::expression::value(doc, &p.id, v, crate::Dimension::Length)
                };
                let width = val(width)?;
                let t = val(thickness)?;
                let mut length = 0.;
                let mut bend_lines = vec![];
                for (i, section) in sections.iter().enumerate() {
                    length += val(section)?;
                    if let Some(bend) = bends.get(i) {
                        let bend = Bend {
                            angle_deg: crate::expression::value(
                                doc,
                                &p.id,
                                &bend.angle,
                                crate::Dimension::Angle,
                            )?
                            .to_degrees(),
                            inside_radius_mm: val(&bend.inside_radius)?,
                            thickness_mm: t,
                            k_factor: bend.k_factor,
                        };
                        let allowance = bend.allowance()?;
                        bend_lines.push(length + allowance * 0.5);
                        length += allowance;
                    }
                }
                patterns.push((f.id.clone(), length, width, bend_lines));
            }
        }
    }
    Ok(patterns)
}
