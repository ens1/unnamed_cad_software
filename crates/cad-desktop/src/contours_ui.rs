//! Docked source-member contour choices over the shared feature draft.
use bevy_egui::egui;
use cad_core::{
    Document, Id, NodeRef, Operation, Profile, Sketch, SketchCurve, contours::Selection,
};

pub(crate) fn sketch<'a>(doc: &'a Document, profile: &NodeRef) -> Option<&'a Sketch> {
    let part = doc.parts.get(&profile.definition)?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == profile.node)?;
    let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &feature.operation
    else {
        return None;
    };
    part.sketches.get(sketch)
}
pub(crate) fn members(doc: &Document, profile: &NodeRef) -> Vec<(Id, String)> {
    let Some(sketch) = sketch(doc, profile) else {
        return vec![];
    };
    let suppressed = cad_core::sketch_transform::suppressed_members(sketch);
    sketch
        .members
        .iter()
        .filter(|(id, member)| !member.construction && !suppressed.contains(*id))
        .enumerate()
        .map(|(index, (id, member))| {
            let kind = match member.curve {
                SketchCurve::Line { .. } => "Line",
                SketchCurve::Arc { .. } => "Arc",
                SketchCurve::Circle { .. } => "Circle",
                SketchCurve::Ellipse { .. } => "Ellipse",
                _ => "Spline",
            };
            (id.clone(), format!("{kind} {}", index + 1))
        })
        .collect()
}
fn profile_name(doc: &Document, profile: &NodeRef) -> String {
    doc.parts
        .get(&profile.definition)
        .and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == profile.node)
                .map(|feature| format!("{} / {}", part.name, feature.name))
        })
        .unwrap_or("Missing Sketch".into())
}
pub(crate) fn fields(
    ui: &mut egui::Ui,
    doc: &Document,
    profiles: &[NodeRef],
    selections: &mut Vec<Selection>,
    thin: bool,
) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Selected Contours").default_open(!selections.is_empty()).show(ui,|ui| {
        if selections.is_empty(){ui.small("All material contours in each source sketch.");}
        let mut remove=None;let mut reorder=None;let count=selections.len();
        for (index,selection) in selections.iter_mut().enumerate(){ui.push_id(index,|ui| {
            if profiles.len()>1||!profiles.contains(&selection.profile) {
                egui::ComboBox::from_label("Source Sketch").selected_text(profile_name(doc,&selection.profile)).show_ui(ui,|ui| {
                    for profile in profiles {changed|=ui.selectable_value(&mut selection.profile,profile.clone(),profile_name(doc,profile)).changed();}
                });
            }
            let choices=members(doc,&selection.profile);
            for member in &mut selection.outer_members {
                let label=choices.iter().find(|(id,_)|id==member).map(|(_,label)|label.clone()).unwrap_or("Missing Material Member".into());
                egui::ComboBox::from_id_salt((index,member.clone())).selected_text(label).show_ui(ui,|ui| {
                    for (id,label) in &choices {changed|=ui.selectable_value(member,id.clone(),label).changed();}
                });
            }
            if selection.outer_members.is_empty(){ui.colored_label(egui::Color32::YELLOW,"Select an outer-boundary member in Advanced Source or remove this empty contour.");}
            ui.horizontal(|ui| {
                if ui.small_button("Remove Contour").clicked(){remove=Some(index);}
                if ui.add_enabled(index>0,egui::Button::new("Move Up").small()).clicked(){reorder=Some((index,index-1));}
                if ui.add_enabled(index+1<count,egui::Button::new("Move Down").small()).clicked(){reorder=Some((index,index+1));}
            });
        });}
        if let Some(index)=remove{selections.remove(index);changed=true;}
        else if let Some((from,to))=reorder{selections.swap(from,to);changed=true;}
        let first=profiles.iter().find_map(|profile|members(doc,profile).first().map(|(member,_)|(profile.clone(),member.clone())));
        if ui.add_enabled(first.is_some(),egui::Button::new("Add Contour")).clicked()&&let Some((profile,member))=first {
            selections.push(Selection {profile,outer_members:vec![member]});changed=true;
        }
        if !selections.is_empty()&&ui.small_button("Use All Contours").clicked(){selections.clear();changed=true;}
        ui.small(if thin {"Choose a member of each connected chain to offset. Open chains receive flat ends."}else {"Choose an outer-boundary member for each region. Contained holes stay with the region."});
        if profiles.len()>1 {ui.small("Selected regions pair in this order for each source sketch. A source sketch with no selection keeps all its material regions.");}
    });
    changed
}
