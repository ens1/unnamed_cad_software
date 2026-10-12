//! View-only contour inspection over bounded, current authoritative sketch source.
use crate::{Workspace, presentation_controls as controls, sketch};
use bevy_egui::egui;
use cad_core::{
    Id, Sketch, SketchCurve,
    presentation::SemanticElement,
    sketch_actions::SketchEntity,
    sketch_topology::{self, ContourStatus},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
};

const ACTION: &str = "sketch.inspect-issue";
const CONTRACT_VERSION: u32 = 1;
const MAX_POINTS: usize = 128;
const MAX_MEMBERS: usize = 128;
const MAX_CONSTRAINTS: usize = 256;
const MAX_SOURCE_BYTES: usize = 256 * 1024;

#[derive(Clone, PartialEq, Eq)]
struct Scope {
    document: Id,
    definition: Id,
    sketch: Id,
    feature: Id,
    editor: Id,
    fingerprint: String,
}
#[derive(Clone)]
struct Issue {
    id: String,
    kind: &'static str,
    label: String,
    help: &'static str,
    members: Vec<Id>,
    names: Vec<String>,
}
#[derive(Clone)]
struct Inspection {
    scope: Scope,
    issues: Vec<Issue>,
    analysis_error: Option<String>,
    limited: bool,
}
thread_local! {
    static CACHE: RefCell<Option<Inspection>> = const { RefCell::new(None) };
}

/// Hash serialization without allocating or cloning an unbounded source snapshot.
struct Fingerprint {
    hash: Sha256,
    bytes: usize,
}
impl Write for Fingerprint {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_SOURCE_BYTES.saturating_sub(self.bytes) {
            return Err(io::Error::other("Sketch inspection source limit exceeded"));
        }
        self.hash.update(bytes);
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn fingerprint(s: &sketch::EditingSketch) -> Option<String> {
    let mut writer = Fingerprint {
        hash: Sha256::new(),
        bytes: 0,
    };
    // These borrowed fields are precisely the source represented by
    // sketch_definition. Transient selection, solve reports and display meshes
    // cannot invalidate or become authority for this connectivity cache.
    serde_json::to_writer(
        &mut writer,
        &(
            (&s.name, &s.keys, &s.points, &s.constraints, &s.members),
            (
                &s.conversions,
                &s.offsets,
                &s.fillets,
                &s.chamfers,
                &s.transforms,
                &s.splines,
                &s.conics,
            ),
            (
                &s.texts,
                &s.equations,
                &s.blocks,
                &s.block_instances,
                &s.dimension_groups,
            ),
        ),
    )
    .ok()?;
    Some(format!("{:x}", writer.hash.finalize()))
}
fn enabled(w: &Workspace) -> bool {
    !w.confirm_requested
        && w.sketch.as_ref().is_some_and(|s| {
            s.drag.is_none()
                && s.chain.is_empty()
                && s.clicks.is_empty()
                && s.edit.is_none()
                && s.dimension.is_none()
                && s.dimension_pick.is_none()
        })
}
fn inspect(w: &Workspace) -> Option<Inspection> {
    let s = w.sketch.as_ref()?;
    if s.choosing_plane || w.settings.is_some() || w.view.history_cursor.is_some() {
        return None;
    }
    let bounded = s.points.len() <= MAX_POINTS
        && s.keys.len() <= MAX_POINTS
        && s.members.len() <= MAX_MEMBERS
        && s.constraints.len() <= MAX_CONSTRAINTS;
    let fingerprint = bounded.then(|| fingerprint(s)).flatten();
    let scope = Scope {
        document: w.session.visible().id.clone(),
        definition: s.definition.clone(),
        sketch: s.sketch_id.clone(),
        feature: s.feature.clone(),
        editor: s.feedback.analysis_token.clone(),
        fingerprint: fingerprint.clone().unwrap_or_default(),
    };
    if fingerprint.is_none() {
        // No actionable finding exists when bounded source inspection is omitted.
        return Some(Inspection {
            scope,
            issues: vec![],
            analysis_error: None,
            limited: true,
        });
    }
    CACHE.with(|cache| {
        if let Some(inspection) = cache.borrow().as_ref()
            && inspection.scope == scope
        {
            return Some(inspection.clone());
        }
        let source = sketch::sketch_definition(s);
        let mut inspection = Inspection {
            scope,
            issues: vec![],
            analysis_error: None,
            limited: false,
        };
        collect(&source, &mut inspection);
        *cache.borrow_mut() = Some(inspection.clone());
        Some(inspection)
    })
}
fn member_name(source: &Sketch, id: &str) -> String {
    let index = source.members.keys().position(|key| key == id).unwrap_or(0) + 1;
    let kind = match source.members.get(id).map(|member| &member.curve) {
        Some(SketchCurve::Line { .. }) => "Line",
        Some(SketchCurve::Arc { .. }) => "Arc",
        Some(SketchCurve::Circle { .. }) => "Circle",
        Some(SketchCurve::Ellipse { .. }) => "Ellipse",
        Some(SketchCurve::Bezier { .. } | SketchCurve::Nurbs { .. }) => "Spline",
        None => "Member",
    };
    format!("{kind} {index}")
}
fn issue(
    source: &Sketch,
    kind: &'static str,
    label: &str,
    help: &'static str,
    members: impl IntoIterator<Item = Id>,
) -> Issue {
    let members = members.into_iter().collect::<BTreeSet<_>>();
    let mut hash = Sha256::new();
    hash.update(kind.as_bytes());
    for id in &members {
        hash.update((id.len() as u64).to_le_bytes());
        hash.update(id.as_bytes());
    }
    Issue {
        id: format!("{:x}", hash.finalize()),
        kind,
        label: format!(
            "{label} · {} {}",
            members.len(),
            if members.len() == 1 {
                "member"
            } else {
                "members"
            }
        ),
        help,
        names: members.iter().map(|id| member_name(source, id)).collect(),
        members: members.into_iter().collect(),
    }
}
fn coordinate(point: [f64; 2]) -> Option<[u64; 2]> {
    point.iter().all(|value| value.is_finite()).then(|| {
        point.map(|value| {
            // Signed zero identifies the same exact authored location.
            if value == 0. { 0 } else { value.to_bits() }
        })
    })
}
fn collect(source: &Sketch, inspection: &mut Inspection) {
    if source
        .points
        .values()
        .flatten()
        .any(|value| !value.is_finite())
    {
        inspection.analysis_error =
            Some("Current sketch endpoint controls must be finite for contour inspection".into());
        return;
    }
    let topology = sketch_topology::analyze(source, sketch_topology::DEFAULT_TOLERANCE_MM);
    if let Ok(topology) = &topology {
        for contour in &topology.contours {
            if contour.status == ContourStatus::Branched {
                inspection.issues.push(issue(
                    source,
                    "branched_contour",
                    "Branched contour",
                    "Inspect the connected material members. At least one section vertex has more than two incident members; remove, trim or make unwanted members construction before using this contour as material.",
                    contour.members.iter().cloned(),
                ));
            }
        }
    } else if let Err(error) = &topology {
        inspection.analysis_error = Some(error.to_string());
    }
    let suppressed = cad_core::sketch_transform::suppressed_members(source);
    let mut exact = BTreeMap::<([u64; 2], [u64; 2]), BTreeSet<Id>>::new();
    let mut canonical = BTreeMap::<(Id, Id), BTreeSet<Id>>::new();
    let mut endpoints = BTreeMap::<[u64; 2], BTreeSet<Id>>::new();
    for (id, member) in &source.members {
        if member.construction || suppressed.contains(id) {
            continue;
        }
        let Ok(SketchCurve::Line { start, end }) =
            cad_core::sketch_edit::effective_curve(source, id)
        else {
            continue;
        };
        let positions = source
            .points
            .get(&start)
            .copied()
            .and_then(coordinate)
            .zip(source.points.get(&end).copied().and_then(coordinate));
        if let Some((a, b)) = positions {
            for p in [a, b] {
                endpoints.entry(p).or_default().insert(id.clone());
            }
            if a == b {
                inspection.issues.push(issue(
                    source,
                    "collapsed_line",
                    "Collapsed line",
                    "This material line has identical finite authored endpoint locations. Inspection selects it without changing geometry or constraints.",
                    [id.clone()],
                ));
            } else {
                let pair = if a <= b { (a, b) } else { (b, a) };
                exact.entry(pair).or_default().insert(id.clone());
            }
        }
        if let Ok(topology) = &topology
            && let Some((a, b)) = topology
                .point_aliases
                .get(&start)
                .zip(topology.point_aliases.get(&end))
            && a != b
        {
            let pair = if a <= b {
                (a.clone(), b.clone())
            } else {
                (b.clone(), a.clone())
            };
            canonical.entry(pair).or_default().insert(id.clone());
        }
    }
    let mut duplicates = BTreeSet::<BTreeSet<Id>>::new();
    for members in exact.into_values().filter(|members| members.len() > 1) {
        inspection.issues.push(issue(
            source,
            "duplicate_line_endpoints",
            "Duplicate line endpoints",
            "These straight material members use the same exact finite endpoint locations in either direction. Inspection highlights every member; delete or make unwanted members construction explicitly.",
            members.iter().cloned(),
        ));
        duplicates.insert(members);
    }
    for members in canonical.into_values().filter(|members| members.len() > 1) {
        if duplicates.insert(members.clone()) {
            inspection.issues.push(issue(
                source,
                "shared_section_endpoints",
                "Shared section endpoints",
                "These straight material members share both section endpoint identities through authored connections, pattern provenance or bounded isolated endpoint healing. This describes section connectivity; it does not prove exact geometric duplication.",
                members,
            ));
        }
    }
    for members in endpoints.into_values().filter(|members| members.len() > 2) {
        let covered = inspection.issues.iter().any(|finding| {
            matches!(
                finding.kind,
                "branched_contour" | "coincident_line_endpoints"
            ) && members.iter().all(|id| finding.members.contains(id))
        });
        if !covered {
            inspection.issues.push(issue(
                source,
                "coincident_line_endpoints",
                "Coincident line endpoints",
                "Three or more distinct material lines meet at the same exact finite authored endpoint location. Separate point IDs can prevent this ambiguous cluster from becoming a joined section vertex. Inspect and edit the intended connectivity explicitly.",
                members,
            ));
        }
    }
}
fn prefix(scope: &Scope) -> String {
    format!("sketch-issues:{}/{}", scope.definition, scope.sketch)
}
fn arguments(inspection: &Inspection, finding: &Issue) -> Value {
    let scope = &inspection.scope;
    json!({"contract":"tcad.sketch-contour-inspection","version":CONTRACT_VERSION,
        "document":scope.document,"definition":scope.definition,"sketch":scope.sketch,
        "feature":scope.feature,"editor":scope.editor,"source_fingerprint":scope.fingerprint,
        "issue":finding.id,"kind":finding.kind,"members":finding.members})
}
fn element(w: &Workspace, inspection: &Inspection, finding: &Issue) -> SemanticElement {
    let mut data = arguments(inspection, finding);
    data["owner"] = json!(inspection.scope.sketch);
    data["classes"] = json!(["sketch-inspection", "contour-issue"]);
    data["member_names"] = json!(finding.names);
    data["description"] = json!(finding.help);
    data["source_mutation"] = json!(false);
    data["native_validation"] = json!(false);
    data["actions"] = json!([{"command":ACTION,"label":"Inspect","enabled":enabled(w),
        "arguments":arguments(inspection,finding)}]);
    let selected = w.sketch.as_ref().is_some_and(|s| {
        s.selection.len() == finding.members.len()
            && finding.members.iter().all(|member| {
                s.selection.contains(&SketchEntity::Member {
                    member: member.clone(),
                })
            })
    });
    SemanticElement {
        id: format!("{}/{}", prefix(&inspection.scope), finding.id),
        role: "sketch-issue".into(),
        label: finding.label.clone(),
        value: None,
        state: if !enabled(w) {
            "disabled"
        } else if selected {
            "selected"
        } else {
            "available"
        }
        .into(),
        commands: vec![ACTION.into()],
        relationships: std::iter::once(inspection.scope.sketch.clone())
            .chain(finding.members.iter().cloned())
            .collect(),
        data,
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(inspection) = inspect(w) else {
        return vec![];
    };
    let mut elements = vec![SemanticElement {
        id: format!("{}/summary", prefix(&inspection.scope)),
        role: "sketch-inspection-summary".into(),
        label: "Contour inspection".into(),
        value: Some(if inspection.limited {
            "Bounded inspection omitted for this sketch size".into()
        } else {
            format!("{} findings", inspection.issues.len())
        }),
        state: if inspection.limited {
            "limited"
        } else if inspection.analysis_error.is_some() {
            "unavailable"
        } else if inspection.issues.is_empty() {
            "ready"
        } else {
            "attention"
        }
        .into(),
        commands: vec![],
        relationships: vec![inspection.scope.sketch.clone()],
        data: json!({"contract":"tcad.sketch-contour-inspection","version":CONTRACT_VERSION,
            "owner":inspection.scope.sketch,"source_fingerprint":inspection.scope.fingerprint,
            "collection_role":"sketch-issue","findings":inspection.issues.len(),"limited":inspection.limited,
            "analysis_error":inspection.analysis_error,"native_validation":false,"source_mutation":false,
            "scope":"authored-material-connectivity-and-line-endpoints",
            "connectivity_tolerance_mm":sketch_topology::DEFAULT_TOLERANCE_MM,
            "maximum_points":MAX_POINTS,"maximum_members":MAX_MEMBERS,"maximum_constraints":MAX_CONSTRAINTS,
            "maximum_source_bytes":MAX_SOURCE_BYTES}),
    }];
    elements.extend(
        inspection
            .issues
            .iter()
            .map(|finding| element(w, &inspection, finding)),
    );
    elements
}
pub fn controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let Some(inspection) = inspect(w).filter(|inspection| !inspection.issues.is_empty()) else {
        return;
    };
    controls::label(
        ui,
        &format!("{}/summary", prefix(&inspection.scope)),
        "sketch-inspection-summary",
        &["sketch-inspection"],
        "Contour inspection",
        "attention",
    );
    for finding in &inspection.issues {
        let semantic = element(w, &inspection, finding);
        if controls::button(
            ui,
            &semantic.id,
            &semantic.role,
            &["sketch-inspection", "contour-issue"],
            &format!("Inspect {}", finding.label),
            enabled(w),
            semantic.state == "selected",
        )
        .on_hover_text(format!(
            "{}\n{}{}",
            finding.help,
            finding.names.join(", "),
            if enabled(w) {
                ""
            } else {
                "\nFinish the current drawing, dimension or editing gesture before inspection."
            }
        ))
        .clicked()
        {
            action(w, ACTION, &semantic.data);
        }
    }
}
/// Select only disposable view state. No source, pending dimension checkpoint,
/// draft evaluation, undo entry or worker request is changed by inspection.
pub fn action(w: &mut Workspace, command: &str, args: &Value) -> bool {
    if command != ACTION {
        return false;
    }
    let Some(inspection) = inspect(w) else {
        w.status = "Open the current sketch before inspecting its members".into();
        return true;
    };
    let finding = inspection.issues.iter().find(|finding| {
        args["contract"] == "tcad.sketch-contour-inspection"
            && args["version"] == CONTRACT_VERSION
            && args["document"] == inspection.scope.document
            && args["definition"] == inspection.scope.definition
            && args["sketch"] == inspection.scope.sketch
            && args["feature"] == inspection.scope.feature
            && args["editor"] == inspection.scope.editor
            && args["source_fingerprint"] == inspection.scope.fingerprint
            && args["issue"] == finding.id
            && args["kind"] == finding.kind
            && args["members"] == json!(finding.members)
    });
    let Some(finding) = finding else {
        w.status =
            "This contour finding is stale; inspect a finding from the current sketch".into();
        return true;
    };
    if !enabled(w) {
        w.status = "Finish the current sketch gesture before inspecting contour members".into();
        return true;
    }
    if let Some(s) = &mut w.sketch {
        s.selection = finding
            .members
            .iter()
            .map(|member| SketchEntity::Member {
                member: member.clone(),
            })
            .collect();
        s.selected_member = finding.members.first().cloned();
        s.selected = None;
        s.dimension_model = None;
    }
    w.selected = None;
    w.geometry_selection.clear();
    w.context_selection = None;
    w.selected_feature_path = None;
    w.selection_popup = None;
    w.candidates.clear();
    w.candidate_index = 0;
    w.status = format!(
        "{} highlighted; edit unwanted members or their connections explicitly",
        finding.label
    );
    true
}
