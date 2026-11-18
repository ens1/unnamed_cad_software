//! AC1027 drawing sheets arranged side by side in millimetres.
use super::{Primitive, Sheet, sheets};
use crate::{Document, Result, protocol::Evaluation, sketch_edit::Curve2};
use std::collections::BTreeSet;

struct Writer {
    data: String,
    next_handle: usize,
}

impl Writer {
    fn handle(&mut self) -> usize {
        let id = self.next_handle;
        self.next_handle += 1;
        id
    }

    fn record(&mut self, kind: &str, handle: usize, owner: usize, class: &str) {
        self.data.push_str(&format!(
            "0\n{kind}\n5\n{handle:X}\n330\n{owner:X}\n100\n{class}\n"
        ));
    }

    fn table(&mut self, kind: &str, count: usize) -> usize {
        let id = self.handle();
        self.data.push_str(&format!(
            "0\nTABLE\n2\n{kind}\n5\n{id:X}\n330\n0\n100\nAcDbSymbolTable\n70\n{count}\n"
        ));
        id
    }

    fn table_entry(&mut self, kind: &str, owner: usize, class: &str, name: &str) -> usize {
        let id = self.handle();
        self.record(kind, id, owner, "AcDbSymbolTableRecord");
        self.data.push_str(&format!("100\n{class}\n2\n{name}\n"));
        id
    }

    fn tables_and_blocks(&mut self, layers: &[String]) -> usize {
        self.data.push_str("0\nSECTION\n2\nTABLES\n");
        let lines = self.table("LTYPE", 5);
        for (name, description, pattern) in [
            ("ByBlock", "", &[][..]),
            ("ByLayer", "", &[][..]),
            ("CONTINUOUS", "Solid line", &[][..]),
            (
                "TCAD_REFERENCE",
                "Reference geometry",
                &crate::drawing_strokes::REFERENCE_PATTERN_MM[..],
            ),
            (
                "TCAD_HIDDEN",
                "Hidden model edges",
                &crate::drawing_strokes::HIDDEN_PATTERN_MM[..],
            ),
        ] {
            self.table_entry("LTYPE", lines, "AcDbLinetypeTableRecord", name);
            let length = pattern.iter().sum::<f64>();
            self.data.push_str(&format!(
                "70\n0\n3\n{description}\n72\n65\n73\n{}\n40\n{length}\n",
                pattern.len()
            ));
            for (index, &length) in pattern.iter().enumerate() {
                let element = if index % 2 == 0 { length } else { -length };
                self.data.push_str(&format!("49\n{element}\n74\n0\n"));
            }
        }
        self.data.push_str("0\nENDTAB\n");
        let table = self.table("LAYER", layers.len() + 1);
        for layer in std::iter::once("0").chain(layers.iter().map(String::as_str)) {
            self.table_entry("LAYER", table, "AcDbLayerTableRecord", layer);
            self.data.push_str("70\n0\n62\n7\n6\nCONTINUOUS\n");
        }
        self.data.push_str("0\nENDTAB\n");
        let table = self.table("BLOCK_RECORD", 2);
        let mut blocks = vec![];
        for name in ["*Model_Space", "*Paper_Space"] {
            let id = self.table_entry("BLOCK_RECORD", table, "AcDbBlockTableRecord", name);
            self.data.push_str("70\n0\n280\n0\n281\n0\n");
            blocks.push((id, name));
        }
        self.data
            .push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n");
        for &(owner, name) in &blocks {
            let start = self.handle();
            self.record("BLOCK", start, owner, "AcDbEntity");
            self.data.push_str(&format!(
                "8\n0\n100\nAcDbBlockBegin\n2\n{name}\n70\n0\n10\n0\n20\n0\n30\n0\n3\n{name}\n1\n\n"
            ));
            let end = self.handle();
            self.record("ENDBLK", end, owner, "AcDbEntity");
            self.data.push_str("8\n0\n100\nAcDbBlockEnd\n");
        }
        self.data.push_str("0\nENDSEC\n");
        blocks[0].0
    }
}

struct SheetWriter<'a> {
    writer: &'a mut Writer,
    model_space: usize,
    offset: f64,
    layer: &'a str,
}

impl SheetWriter<'_> {
    fn entity(&mut self, kind: &str, class: &str, linetype: &str) {
        let id = self.writer.handle();
        self.writer.record(kind, id, self.model_space, "AcDbEntity");
        self.writer
            .data
            .push_str(&format!("8\n{}\n6\n{linetype}\n100\n{class}\n", self.layer));
    }

    fn point(&mut self, group: usize, p: [f64; 2]) {
        self.writer.data.push_str(&format!(
            "{group}\n{}\n{}\n{}\n{}\n0\n",
            self.offset + p[0],
            group + 10,
            p[1],
            group + 20
        ));
    }

    fn line(&mut self, a: [f64; 2], b: [f64; 2], linetype: &str) {
        self.entity("LINE", "AcDbLine", linetype);
        self.point(10, a);
        self.point(11, b);
    }

    fn spline(&mut self, curve: &Curve2, linetype: &str) {
        if curve.degree == 1 && curve.control_points.len() == 2 {
            self.line(curve.control_points[0], curve.control_points[1], linetype);
            return;
        }
        self.entity("SPLINE", "AcDbSpline", linetype);
        self.writer.data.push_str(&format!(
            "210\n0\n220\n0\n230\n1\n70\n12\n71\n{}\n72\n{}\n73\n{}\n74\n0\n",
            curve.degree,
            curve.knots.len(),
            curve.control_points.len()
        ));
        for knot in &curve.knots {
            self.writer.data.push_str(&format!("40\n{knot}\n"));
        }
        for weight in &curve.weights {
            self.writer.data.push_str(&format!("41\n{weight}\n"));
        }
        for &p in &curve.control_points {
            self.point(10, p);
        }
    }

    fn label(&mut self, at: [f64; 2], size: f64, text: &str) {
        let text = text.replace(['\n', '\r'], " ");
        if text.len() <= 250 && !text.contains("%%") && !text.contains("\\U+") {
            self.entity("TEXT", "AcDbText", "CONTINUOUS");
            self.point(10, at);
            self.writer
                .data
                .push_str(&format!("40\n{size}\n1\n{text}\n100\nAcDbText\n73\n0\n"));
            return;
        }
        // Long group-1 strings truncate in CAD readers. MTEXT permits chunks
        // and literal escapes so a user's percent codes or braces print verbatim.
        self.entity("MTEXT", "AcDbMText", "CONTINUOUS");
        self.point(10, at);
        self.writer
            .data
            .push_str(&format!("40\n{size}\n41\n0\n71\n7\n"));
        let mut chunk = String::new();
        for ch in text.chars() {
            let mut utf8 = [0u8; 4];
            let token = match ch {
                '\\' => "\\\\",
                '{' => "\\{",
                '}' => "\\}",
                '%' => "\\U+0025",
                _ => ch.encode_utf8(&mut utf8),
            };
            if chunk.len() + token.len() > 250 {
                self.writer.data.push_str(&format!("3\n{chunk}\n"));
                chunk.clear();
            }
            chunk.push_str(token);
        }
        self.writer.data.push_str(&format!("1\n{chunk}\n"));
    }

    fn write(&mut self, sheet: Sheet) -> Result<()> {
        for primitive in sheet.primitives {
            match primitive {
                Primitive::Polyline {
                    points,
                    dashed: true,
                } => {
                    for [a, b] in crate::drawing_strokes::reference_segments(&points)? {
                        self.line(a, b, "CONTINUOUS");
                    }
                }
                Primitive::Polyline {
                    points,
                    dashed: false,
                } => {
                    for p in points.windows(2) {
                        self.line(p[0], p[1], "CONTINUOUS");
                    }
                }
                Primitive::HiddenPolyline { points } => {
                    for p in points.windows(2) {
                        self.line(p[0], p[1], "TCAD_HIDDEN");
                    }
                }
                Primitive::Text { at, size, text } => self.label(at, size, &text),
                Primitive::Spline { curve, dashed } => self.spline(
                    &curve,
                    if dashed {
                        "TCAD_REFERENCE"
                    } else {
                        "CONTINUOUS"
                    },
                ),
                Primitive::HiddenSpline { curve } => self.spline(&curve, "TCAD_HIDDEN"),
                Primitive::Circle { center, radius } => {
                    self.entity("CIRCLE", "AcDbCircle", "CONTINUOUS");
                    self.point(10, center);
                    self.writer.data.push_str(&format!("40\n{radius}\n"));
                }
            }
        }
        Ok(())
    }
}

fn layer_names(sheets: &[Sheet]) -> Vec<String> {
    let mut used = BTreeSet::from(["0".to_string()]);
    sheets
        .iter()
        .enumerate()
        .map(|(i, sheet)| {
            let name = sheet.name.trim();
            if !name.is_empty()
                && name.len() <= 255
                && !name
                    .chars()
                    .any(|c| c.is_control() || "<>/\\\":;?*|=,`".contains(c))
                && used.insert(name.to_lowercase())
            {
                return name.to_owned();
            }
            let mut number = i + 1;
            loop {
                let alternative = format!("Sheet {number}");
                if used.insert(alternative.to_lowercase()) {
                    return alternative;
                }
                number += 1;
            }
        })
        .collect()
}

pub fn dxf(doc: &Document, e: &Evaluation) -> Result<String> {
    let sheets = sheets(doc, e)?;
    let layers = layer_names(&sheets);
    let mut writer = Writer {
        data: String::new(),
        next_handle: 1,
    };
    let model_space = writer.tables_and_blocks(&layers);
    writer.data.push_str("0\nSECTION\n2\nENTITIES\n");
    let mut offset = 0.;
    for (sheet, layer) in sheets.into_iter().zip(&layers) {
        let width = sheet.paper_mm[0];
        SheetWriter {
            writer: &mut writer,
            model_space,
            offset,
            layer,
        }
        .write(sheet)?;
        offset += width + 20.;
    }
    writer.data.push_str("0\nENDSEC\n0\nEOF\n");
    let mut out = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1027\n9\n$INSUNITS\n70\n4\n9\n$HANDSEED\n5\n{:X}\n0\nENDSEC\n",
        writer.next_handle
    );
    out.push_str(&writer.data);
    Ok(out)
}
