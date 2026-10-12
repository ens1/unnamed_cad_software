//! Source-qualified reports from existing cached f64 engineering properties.
//! Reporting frames do not query geometry or alter evaluated CAD definitions.
use super::{Metrics, combine, placed};
use crate::{
    CadError, Id, NodeRef, PlacedPart, Result,
    protocol::{Evaluation, RequestIdentity},
};
use nalgebra::{Matrix3, Matrix4};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io::Write};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_EXPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_ITEMS: usize = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    pub path: Vec<Id>,
    pub definition: Id,
    pub configuration: Option<Id>,
}
impl From<&PlacedPart> for Occurrence {
    fn from(value: &PlacedPart) -> Self {
        Self {
            path: value.path.clone(),
            definition: value.definition.clone(),
            configuration: value.configuration.clone(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Frame {
    #[default]
    World,
    Component {
        occurrence: Occurrence,
    },
    CoordinateSystem {
        occurrence: Occurrence,
        feature: Id,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Assembly,
    Component {
        occurrence: Occurrence,
    },
    Body {
        occurrence: Occurrence,
        body: NodeRef,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub scope: Scope,
    pub frame: Frame,
    pub history: Option<NodeRef>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheState {
    Current,
    LastValid,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub occurrence: Occurrence,
    pub mesh_key: Id,
    pub body: NodeRef,
    pub representation: String,
    pub assembly_weld: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub version: u32,
    pub source: RequestIdentity,
    pub cache_state: CacheState,
    pub evaluation_valid: bool,
    pub geometry_valid: bool,
    pub request: Request,
    pub frame_to_world: [[f64; 4]; 4],
    pub frame_label: String,
    pub bounds_basis: String,
    pub inertia_basis: String,
    pub metrics: Metrics,
    pub items: Vec<Item>,
    pub notes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameChoice {
    pub frame: Frame,
    pub label: String,
}

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Mass Properties: {}", message.into()))
}
pub fn validate_rigid(matrix: [[f64; 4]; 4]) -> Result<()> {
    let rotation = Matrix3::from_fn(|r, c| matrix[r][c]);
    if matrix.iter().flatten().any(|value| !value.is_finite())
        || (0..4)
            .any(|column| (matrix[3][column] - if column == 3 { 1.0 } else { 0.0 }).abs() > 1e-10)
        || (rotation.transpose() * rotation - Matrix3::identity()).norm() > 1e-9
        || (rotation.determinant() - 1.0).abs() > 1e-9
    {
        return Err(invalid(
            "The reporting or occurrence frame is not a finite right-handed rigid transform",
        ));
    }
    Ok(())
}
fn matrix(value: [[f64; 4]; 4]) -> Matrix4<f64> {
    Matrix4::from_fn(|r, c| value[r][c])
}
fn array(value: Matrix4<f64>) -> [[f64; 4]; 4] {
    std::array::from_fn(|r| std::array::from_fn(|c| value[(r, c)]))
}
fn valid_occurrence(value: &Occurrence) -> Result<()> {
    if value.path.len() > 64
        || value.definition.is_empty()
        || value.definition.len() > 1024
        || value.path.iter().any(|id| id.is_empty() || id.len() > 1024)
        || value
            .configuration
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 1024)
    {
        return Err(invalid("Invalid source-qualified occurrence identity"));
    }
    Ok(())
}
fn matches(value: &Occurrence, placement: &PlacedPart) -> bool {
    value.path == placement.path
        && value.definition == placement.definition
        && value.configuration == placement.configuration
}
fn occurrence<'a>(evaluation: &'a Evaluation, value: &Occurrence) -> Result<&'a PlacedPart> {
    valid_occurrence(value)?;
    let mut found = evaluation
        .instances
        .iter()
        .chain(
            evaluation
                .assembly_welds
                .values()
                .map(|weld| &weld.placement),
        )
        .filter(|placement| matches(value, placement));
    let result = found.next().ok_or_else(|| {
        invalid("The selected occurrence is missing or suppressed in this cached configuration")
    })?;
    if found.next().is_some() {
        return Err(invalid("The selected occurrence identity is ambiguous"));
    }
    Ok(result)
}
pub fn resolve_frame(evaluation: &Evaluation, frame: &Frame) -> Result<([[f64; 4]; 4], String)> {
    let (transform, label) = match frame {
        Frame::World => (array(Matrix4::identity()), "World".into()),
        Frame::Component { occurrence: value } => {
            let instance = occurrence(evaluation, value)?;
            (
                instance.matrix,
                format!("{} · component coordinates", instance.name),
            )
        }
        Frame::CoordinateSystem {
            occurrence: value,
            feature,
        } => {
            if feature.is_empty() || feature.len() > 1024 {
                return Err(invalid("Invalid coordinate-system feature identity"));
            }
            let instance = occurrence(evaluation, value)?;
            let key = format!("{}::{feature}", crate::history::mesh_key(instance));
            let datum=evaluation.references.get(&key).filter(|datum|datum.producer.definition==value.definition && datum.producer.node==*feature && (1..=crate::reference_geometry::CONTRACT_VERSION).contains(&datum.version))
                .ok_or_else(||invalid("The source-qualified coordinate system is missing or suppressed in this cached configuration"))?;
            let crate::reference_geometry::Geometry::CoordinateSystem { frame } = datum.geometry
            else {
                return Err(invalid("The chosen reference is not a coordinate system"));
            };
            frame.validate()?;
            let local = Matrix4::new(
                frame.x[0],
                frame.y[0],
                frame.z[0],
                frame.origin[0],
                frame.x[1],
                frame.y[1],
                frame.z[1],
                frame.origin[1],
                frame.x[2],
                frame.y[2],
                frame.z[2],
                frame.origin[2],
                0.,
                0.,
                0.,
                1.,
            );
            (
                array(matrix(instance.matrix) * local),
                format!("{} / {}", instance.name, datum.name),
            )
        }
    };
    validate_rigid(transform)?;
    Ok((transform, label))
}
/// Choices retain actual occurrence paths/configurations, never display-name lookup.
pub fn frame_choices(evaluation: &Evaluation) -> Result<Vec<FrameChoice>> {
    let mut out = vec![FrameChoice {
        frame: Frame::World,
        label: "World".into(),
    }];
    for instance in &evaluation.instances {
        out.push(FrameChoice {
            frame: Frame::Component {
                occurrence: instance.into(),
            },
            label: format!("{} · component", instance.name),
        });
        for (_, datum) in evaluation.references.iter().filter(|(key, datum)| {
            *key == &format!(
                "{}::{}",
                crate::history::mesh_key(instance),
                datum.producer.node
            ) && datum.producer.definition == instance.definition
                && matches!(
                    datum.geometry,
                    crate::reference_geometry::Geometry::CoordinateSystem { .. }
                )
        }) {
            out.push(FrameChoice {
                frame: Frame::CoordinateSystem {
                    occurrence: instance.into(),
                    feature: datum.producer.node.clone(),
                },
                label: format!("{} / {}", instance.name, datum.name),
            });
        }
        if out.len() > 4096 {
            return Err(invalid(
                "Frame picker exceeds 4096 cached choices; select a component to bind its frame directly",
            ));
        }
    }
    Ok(out)
}

pub fn evaluate(
    evaluation: &Evaluation,
    request: &Request,
    cache_state: CacheState,
) -> Result<Report> {
    if !(evaluation.valid || evaluation.geometry_valid) {
        return Err(invalid("No complete cached geometry is available"));
    }
    if evaluation.instances.len() + evaluation.assembly_welds.len() > MAX_ITEMS {
        return Err(invalid("Occurrence reporting budget exceeded"));
    }
    if let Some(history) = &request.history {
        if history.node.is_empty()
            || history.node.len() > 1024
            || !evaluation
                .instances
                .iter()
                .any(|instance| instance.definition == history.definition)
        {
            return Err(invalid(
                "The requested history definition/station is absent from this cached evaluation",
            ));
        }
        if let Frame::CoordinateSystem {
            occurrence: selected,
            feature,
        } = &request.frame
            && selected.definition == history.definition
        {
            let instance = occurrence(evaluation, selected)?;
            let key = crate::history::mesh_key(instance);
            let station = crate::history::stage(evaluation, key, &history.node)
                .ok_or_else(|| invalid("History station is unavailable for the chosen frame"))?;
            let datum = crate::history::stage(evaluation, key, feature)
                .ok_or_else(|| invalid("Coordinate-system history identity is unavailable"))?;
            if datum.index > station.index {
                return Err(invalid(
                    "The coordinate system did not exist at the requested history station",
                ));
            }
        }
    }
    let (frame_to_world, frame_label) = resolve_frame(evaluation, &request.frame)?;
    let world_to_frame = matrix(frame_to_world)
        .try_inverse()
        .ok_or_else(|| invalid("Reporting frame cannot be inverted"))?;
    let selected = match &request.scope {
        Scope::Assembly => None,
        Scope::Component { occurrence: value }
        | Scope::Body {
            occurrence: value, ..
        } => {
            occurrence(evaluation, value)?;
            Some(value)
        }
    };
    let mut values = Vec::new();
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    for instance in &evaluation.instances {
        if selected.is_some_and(|value| !matches(value, instance)) {
            continue;
        }
        if !seen.insert(instance.path.clone()) {
            return Err(invalid(
                "Duplicate cached occurrence paths would double-count geometry",
            ));
        }
        let key = crate::history::mesh_key(instance);
        let properties = evaluation
            .properties
            .get(key)
            .filter(|properties| {
                properties.definition == instance.definition
                    && properties.version == super::CONTRACT_VERSION
            })
            .ok_or_else(|| {
                invalid(format!(
                    "{} has no matching cached properties; partial totals are unavailable",
                    instance.name
                ))
            })?;
        let history = request
            .history
            .as_ref()
            .filter(|history| history.definition == instance.definition);
        let stage = history
            .map(|history| {
                crate::history::stage(evaluation, key, &history.node)
                    .filter(|stage| stage.unavailable_reason.is_none())
                    .ok_or_else(|| {
                        invalid("History station is not available in this cached evaluation")
                    })
            })
            .transpose()?;
        let ids = if let Scope::Body { body, .. } = &request.scope {
            if body.definition != instance.definition {
                return Err(invalid("Selected body belongs to another definition"));
            }
            if stage.is_some_and(|stage| !stage.bodies.contains(&body.node)) {
                return Err(invalid(
                    "Selected body is absent at the displayed history station",
                ));
            }
            vec![body.node.clone()]
        } else if let Some(stage) = stage {
            stage.bodies.clone()
        } else {
            properties.bodies.keys().cloned().collect()
        };
        if ids.is_empty() {
            return Err(invalid(format!(
                "{} has no cached reportable bodies in this scope",
                instance.name
            )));
        }
        let mut body_ids = BTreeSet::new();
        for id in ids {
            if !body_ids.insert(id.clone()) {
                return Err(invalid(
                    "Duplicate cached body identities would double-count geometry",
                ));
            }
            let metrics=properties.bodies.get(&id).ok_or_else(||invalid(format!("Cached properties for {} body {id} are unavailable in this evaluated scope; substituting other body totals is unavailable",instance.name)))?;
            values.push(placed(
                metrics,
                array(world_to_frame * matrix(instance.matrix)),
            )?);
            items.push(Item {
                occurrence: instance.into(),
                mesh_key: key.into(),
                body: NodeRef {
                    definition: instance.definition.clone(),
                    node: id,
                },
                representation: properties.representation.clone(),
                assembly_weld: false,
            });
        }
    }
    if request.history.is_none() {
        for weld in evaluation.assembly_welds.values() {
            if selected.is_some_and(|value| !matches(value, &weld.placement)) {
                continue;
            }
            if weld.version != 1
                || weld.owner.definition != weld.placement.definition
                || !seen.insert(weld.placement.path.clone())
            {
                return Err(invalid(
                    "Assembly deposited-metal cache identity is inconsistent or duplicated",
                ));
            }
            if let Scope::Body { body, .. } = &request.scope
                && body != &weld.owner
            {
                return Err(invalid(
                    "Selected weld body differs from its cached deposited metal",
                ));
            }
            values.push(placed(
                &weld.properties,
                array(world_to_frame * matrix(weld.placement.matrix)),
            )?);
            items.push(Item {
                occurrence: (&weld.placement).into(),
                mesh_key: crate::history::mesh_key(&weld.placement).into(),
                body: weld.owner.clone(),
                representation: "evaluated_boundary_tessellation".into(),
                assembly_weld: true,
            });
        }
    } else if selected.is_some_and(|value| {
        evaluation
            .assembly_welds
            .values()
            .any(|weld| matches(value, &weld.placement))
    }) {
        return Err(invalid(
            "Assembly deposited-metal history properties are unavailable",
        ));
    }
    if items.len() > MAX_ITEMS {
        return Err(invalid("Body reporting budget exceeded"));
    }
    let metrics = combine(&values)?;
    let mut notes=vec!["Values derive from cached boundary tessellations; exact B-rep integration and integration error bounds are unavailable.".into()];
    notes.push("Frames and totals use evaluated physical occurrence placements.".into());
    if matches!(request.scope, Scope::Assembly) {
        notes.push("Assembly totals sum independently owned bodies and deposits. Overlapping occurrences contribute separately.".into());
    }
    if request.history.is_some() {
        notes.push("History uses only cached stage body properties. Assembly weld deposits are hidden in history view and excluded explicitly.".into());
    }
    if metrics.volume_mm3 > 1e-18 && metrics.mass_kg.is_none() {
        notes.push("Mass and inertia are unavailable because at least one solid has no complete density-derived mass properties; area and volume include every reported body.".into());
    }
    Ok(Report {
        version: CONTRACT_VERSION,
        source: evaluation.identity.clone(),
        cache_state,
        evaluation_valid: evaluation.valid,
        geometry_valid: evaluation.geometry_valid,
        request: request.clone(),
        frame_to_world,
        frame_label,
        bounds_basis: "conservative_transformed_cached_body_AABB_corners_not_tight_native_extents"
            .into(),
        inertia_basis: "centroidal_tensor_in_reporting_frame_kg_m2".into(),
        metrics,
        items,
        notes,
    })
}
pub fn export_json(report: &Report) -> Result<String> {
    struct Bounded {
        bytes: Vec<u8>,
        exceeded: bool,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_EXPORT_BYTES.saturating_sub(self.bytes.len()) {
                self.exceeded = true;
                return Err(std::io::Error::other(
                    "Bounded mass-property report export limit",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    if report.items.len() > MAX_ITEMS {
        return Err(invalid("Body reporting budget exceeded"));
    }
    super::validate_metrics(&report.metrics)?;
    validate_rigid(report.frame_to_world)?;
    let mut output = Bounded {
        bytes: Vec::new(),
        exceeded: false,
    };
    let result = serde_json::to_writer_pretty(&mut output, report);
    if output.exceeded {
        return Err(invalid("Report JSON exceeds the 16 MiB export limit"));
    }
    result?;
    String::from_utf8(output.bytes).map_err(|_| invalid("Report serializer produced invalid UTF-8"))
}
