use super::{Origin, Shape};
use crate::{
    Dimension, Expr, Feature, Operation, Parameter, Part, Plane, Profile, Result, Sketch,
    SketchConstraint, SketchCurve, SketchMember,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
fn stable(owner: &str, label: &str) -> String {
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("tcad.hardware.v1:{owner}:{label}"))
    );
    format!(
        "{}-{}-{}-{}-{}",
        &digest[..8],
        &digest[8..12],
        &digest[12..16],
        &digest[16..20],
        &digest[20..32]
    )
}
fn length(value: f64) -> Expr {
    Expr(format!("{value} mm"))
}
fn sketch(part: &Part, label: &str) -> Sketch {
    Sketch {
        id: stable(&part.id, label),
        name: label.replace('-', " "),
        points: BTreeMap::new(),
        segments: vec![],
        constraints: vec![],
        profile_order: vec![],
        circles: BTreeMap::new(),
        members: BTreeMap::new(),
        conversions: BTreeMap::new(),
        offsets: BTreeMap::new(),
        fillets: BTreeMap::new(),
        chamfers: BTreeMap::new(),
        transforms: BTreeMap::new(),
        splines: BTreeMap::new(),
        conics: BTreeMap::new(),
        texts: BTreeMap::new(),
        equations: BTreeMap::new(),
        blocks: BTreeMap::new(),
        block_instances: BTreeMap::new(),
        dimension_groups: BTreeMap::new(),
    }
}
fn point(sketch: &mut Sketch, name: &str, value: [f64; 2]) -> String {
    let id = stable(&sketch.id, name);
    sketch.points.insert(id.clone(), value);
    for (axis, value) in ["x", "y"].into_iter().zip(value) {
        sketch.constraints.push(SketchConstraint {
            id: stable(&id, axis),
            kind: format!("fix_{axis}"),
            entities: vec![id.clone()],
            value: length(value),
            driven: false,
            contact: None,
            distance_side: None,
        });
    }
    id
}
fn circle(sketch: &mut Sketch, label: &str, diameter: f64) {
    let center = point(sketch, &format!("{label}-center"), [0.0; 2]);
    sketch.members.insert(
        stable(&sketch.id, label),
        SketchMember {
            construction: false,
            curve: SketchCurve::Circle {
                center,
                radius: length(diameter / 2.0),
            },
        },
    );
}
fn hexagon(sketch: &mut Sketch, label: &str, across_flats: f64) {
    let radius = across_flats / 3f64.sqrt();
    let mut points = vec![];
    for i in 0..6 {
        let angle = i as f64 * std::f64::consts::TAU / 6.0;
        let coordinates = [radius * angle.cos(), radius * angle.sin()]
            .map(|value| if value.abs() < 1e-12 { 0.0 } else { value });
        points.push(point(sketch, &format!("{label}-vertex-{i}"), coordinates));
    }
    for i in 0..6 {
        sketch.members.insert(
            stable(&sketch.id, &format!("{label}-edge-{i}")),
            SketchMember {
                construction: false,
                curve: SketchCurve::Line {
                    start: points[i].clone(),
                    end: points[(i + 1) % 6].clone(),
                },
            },
        );
    }
}
fn profile(part: &mut Part, sketch: Sketch, z: f64) -> String {
    let id = stable(&sketch.id, "profile");
    let name = sketch.name.clone();
    let sketch_id = sketch.id.clone();
    part.sketches.insert(sketch_id.clone(), sketch);
    part.features.push(Feature {
        id: id.clone(),
        name,
        suppressed: false,
        operation: Operation::Profile {
            profile: Profile::Sketch { sketch: sketch_id },
            plane: Plane {
                origin: [0.0, 0.0, z],
                ..Default::default()
            },
            plane_reference: None,
            reference_body: None,
        },
    });
    id
}
fn extrude(
    part: &mut Part,
    label: &str,
    profile: &str,
    distance: Expr,
    sign: f64,
    body: Option<&str>,
    mode: &str,
) -> String {
    let id = stable(&part.id, label);
    part.features.push(Feature {
        id: id.clone(),
        name: label.replace('-', " "),
        suppressed: false,
        operation: Operation::Extrude {
            profile: part.reference(profile),
            distance,
            direction: [0.0, 0.0, sign],
            body: body.map(|body| part.reference(body)),
            mode: mode.into(),
            options: Default::default(),
        },
    });
    part.outputs = vec![id.clone()];
    id
}
pub(super) fn build(
    origin: &Origin,
    definition: &str,
    material: &str,
    finish: &str,
) -> Result<Part> {
    let family = origin.entry.shape.family();
    let suffix = origin
        .length_mm
        .map(|value| format!(" × {value} mm"))
        .unwrap_or_default();
    let mut part = Part::new(&format!(
        "{} {}{}",
        origin.entry.size,
        family.label(),
        suffix
    ));
    part.id = definition.into();
    part.material = material.into();
    part.finish = finish.into();
    part.standard_part = Some(origin.clone());
    part.properties.insert(
        "part_number".into(),
        format!(
            "TCAD:{}{}",
            origin.entry.id,
            origin
                .length_mm
                .map(|length| format!("-L{length}"))
                .unwrap_or_default()
        ),
    );
    part.properties.insert(
        "description".into(),
        format!(
            "{} {}{} · {} · simplified CAD envelope",
            origin.entry.size,
            family.label(),
            suffix,
            origin.entry.standard_reference
        ),
    );
    part.properties.insert(
        "standard_reference".into(),
        origin.entry.standard_reference.clone(),
    );
    part.properties.insert(
        "representation".into(),
        if family.has_length() {
            "Smooth nominal-diameter shank; threads, runout, tip chamfer and edge blends omitted"
        } else if matches!(origin.entry.shape, Shape::HexNut { .. }) {
            "Straight hex envelope; nominal-diameter smooth bore; threads and face chamfers omitted"
        } else {
            "Nominal Form A washer; no edge breaks"
        }
        .into(),
    );
    part.properties.insert("mass_basis".into(),"Evaluated simplified CAD envelope with the assigned engineering density; no supplier mass or strength certification".into());
    let d = origin.entry.nominal_diameter_mm;
    match origin.entry.shape {
        Shape::FlatWasher {
            inner_diameter_mm,
            outer_diameter_mm,
            thickness_mm,
        } => {
            let mut s = sketch(&part, "Washer-Sketch");
            circle(&mut s, "outer", outer_diameter_mm);
            circle(&mut s, "inner", inner_diameter_mm);
            let source = profile(&mut part, s, 0.0);
            extrude(
                &mut part,
                "Washer-Extrude",
                &source,
                length(thickness_mm),
                1.0,
                None,
                "new",
            );
        }
        Shape::HexNut {
            across_flats_mm,
            height_mm,
            pitch_mm,
        } => {
            part.properties.insert(
                "thread_designation".into(),
                format!("{} × {pitch_mm} · not modeled", origin.entry.size),
            );
            let mut s = sketch(&part, "Nut-Sketch");
            hexagon(&mut s, "outer", across_flats_mm);
            circle(&mut s, "smooth-thread-envelope", d);
            let source = profile(&mut part, s, 0.0);
            extrude(
                &mut part,
                "Nut-Extrude",
                &source,
                length(height_mm),
                1.0,
                None,
                "new",
            );
        }
        Shape::SocketScrew {
            head_diameter_mm,
            height_mm,
            socket_across_flats_mm,
            socket_depth_mm,
            pitch_mm,
        } => {
            part.properties.insert(
                "thread_designation".into(),
                format!("{} × {pitch_mm} · not modeled", origin.entry.size),
            );
            let shank = shank(&mut part, origin.length_mm.unwrap_or_default(), d);
            let mut s = sketch(&part, "Head-Sketch");
            circle(&mut s, "outer", head_diameter_mm);
            let source = profile(&mut part, s, 0.0);
            let head = extrude(
                &mut part,
                "Head-Extrude",
                &source,
                length(height_mm),
                1.0,
                Some(&shank),
                "join",
            );
            let mut s = sketch(&part, "Socket-Sketch");
            hexagon(&mut s, "socket", socket_across_flats_mm);
            let source = profile(&mut part, s, height_mm);
            extrude(
                &mut part,
                "Socket-Cut",
                &source,
                length(socket_depth_mm),
                -1.0,
                Some(&head),
                "cut",
            );
        }
        Shape::HexScrew {
            across_flats_mm,
            height_mm,
            pitch_mm,
        } => {
            part.properties.insert(
                "thread_designation".into(),
                format!("{} × {pitch_mm} · not modeled", origin.entry.size),
            );
            let shank = shank(&mut part, origin.length_mm.unwrap_or_default(), d);
            let mut s = sketch(&part, "Head-Sketch");
            hexagon(&mut s, "outer", across_flats_mm);
            let source = profile(&mut part, s, 0.0);
            extrude(
                &mut part,
                "Head-Extrude",
                &source,
                length(height_mm),
                1.0,
                Some(&shank),
                "join",
            );
        }
    }
    Ok(part)
}
fn shank(part: &mut Part, under_head_length: f64, diameter: f64) -> String {
    part.parameters.insert(
        "length".into(),
        Parameter {
            name: "Under-head length".into(),
            expression: length(under_head_length),
            dimension: Dimension::Length,
        },
    );
    let mut s = sketch(part, "Shank-Sketch");
    circle(&mut s, "outer", diameter);
    let source = profile(part, s, 0.0);
    extrude(
        part,
        "Shank-Extrude",
        &source,
        "length".into(),
        -1.0,
        None,
        "new",
    )
}
