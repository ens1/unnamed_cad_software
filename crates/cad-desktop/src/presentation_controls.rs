//! Replaceable native control adapter over semantic identity and resolved style.
//!
//! The frame snapshot contains presentation resources only. Controls never acquire
//! document or transaction access through it: their caller still owns commands.
use bevy_egui::egui::{self, Color32, FontId, Response, Stroke};
use cad_core::presentation::{StyleTarget, Theme};
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Clone, Default)]
struct FrameStyles {
    theme: Theme,
    layout: String,
    target: Option<StyleTarget>,
}
thread_local! {
    static STYLES: RefCell<FrameStyles> = RefCell::new(FrameStyles::default());
}

pub fn begin_frame(theme: &Theme, layout: &str) {
    STYLES.with(|snapshot| {
        *snapshot.borrow_mut() = FrameStyles {
            theme: theme.clone(),
            layout: layout.into(),
            target: None,
        };
    });
}

pub fn current_target() -> Option<StyleTarget> {
    STYLES.with(|snapshot| snapshot.borrow().target.clone())
}

/// Scope native descendants to their actual layout ancestry. A duplicated
/// component receives the ancestry of the copy being rendered.
pub fn with_target<R>(
    target: StyleTarget,
    ui: &mut egui::Ui,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    struct Restore(Option<StyleTarget>);
    impl Drop for Restore {
        fn drop(&mut self) {
            STYLES.with(|snapshot| snapshot.borrow_mut().target = self.0.take());
        }
    }
    let previous = STYLES.with(|snapshot| snapshot.borrow_mut().target.replace(target));
    let _restore = Restore(previous);
    content(ui)
}

pub fn properties(id: &str, role: &str, classes: &[&str], state: &str) -> BTreeMap<String, String> {
    STYLES.with(|snapshot| {
        let snapshot = snapshot.borrow();
        let parent = snapshot.target.clone().unwrap_or_else(|| {
            cad_core::presentation::Region {
                id: "workspace".into(),
                role: "workspace".into(),
                component: None,
                rect: [0.; 4],
                classes: vec![],
                resizable: false,
            }
            .style_target(&snapshot.layout, "ready")
        });
        let target = if parent.node.id == id {
            let mut target = parent;
            target.node.role = role.into();
            for class in classes {
                if !target.node.classes.iter().any(|existing| existing == class) {
                    target.node.classes.push((*class).into());
                }
            }
            target.node.states.retain(|state| {
                !matches!(
                    state.as_str(),
                    "ready" | "available" | "disabled" | "selected" | "hover" | "focus" | "focused"
                )
            });
            target
                .node
                .states
                .extend(state.split_whitespace().map(str::to_owned));
            target
        } else {
            parent.child(
                id,
                role,
                &classes.iter().map(|c| (*c).into()).collect::<Vec<_>>(),
                state,
            )
        };
        snapshot.theme.properties_for(&target)
    })
}

pub fn number(properties: &BTreeMap<String, String>, key: &str, fallback: f32) -> f32 {
    properties
        .get(key)
        .and_then(|value| value.trim().trim_end_matches("px").parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
}

pub(crate) fn length(
    properties: &BTreeMap<String, String>,
    key: &str,
    extent: f32,
    fallback: f32,
) -> f32 {
    properties
        .get(key)
        .and_then(|value| value.trim().strip_suffix('%'))
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map(|value| extent * value / 100.)
        .unwrap_or_else(|| number(properties, key, fallback))
}

/// Value-widget dimensions resolve against the actual styled parent region.
pub(crate) fn field_size(ui: &egui::Ui, props: &BTreeMap<String, String>) -> egui::Vec2 {
    let available = if ui.available_width().is_finite() {
        ui.available_width().max(0.)
    } else {
        ui.clip_rect().width().max(0.)
    };
    let target = number(props, "min-target-size", 0.).max(0.);
    // CSS minimum sizes request usable space; they cannot manufacture space
    // outside a docked panel. Responsive field layout provides a full-width row
    // when the preferred horizontal composition cannot fit.
    let min_width = length(props, "min-width", available, target)
        .max(target)
        .min(available);
    let max_width = length(props, "max-width", available, available)
        .max(min_width)
        .min(available);
    let width = length(props, "width", available, available).clamp(min_width, max_width);
    let extent = if ui.available_height().is_finite() {
        ui.available_height()
    } else {
        ui.clip_rect().height().max(ui.spacing().interact_size.y)
    };
    let min_height = length(props, "min-height", extent, target).max(target);
    let max_height = length(props, "max-height", extent, f32::INFINITY).max(min_height);
    let height =
        length(props, "height", extent, ui.spacing().interact_size.y).clamp(min_height, max_height);
    egui::vec2(width, height)
}

pub(crate) fn caption_width(
    ui: &egui::Ui,
    props: &BTreeMap<String, String>,
    text: &str,
    available: f32,
) -> f32 {
    if props.get("display").is_some_and(|value| value == "none") {
        return 0.;
    }
    let measured = ui
        .painter()
        .layout_no_wrap(text.into(), font(ui, props), Color32::WHITE)
        .size()
        .x;
    let minimum = length(props, "min-width", available, 0.)
        .max(0.)
        .min(available);
    let maximum = length(props, "max-width", available, available)
        .max(minimum)
        .min(available);
    length(props, "width", available, measured).clamp(minimum, maximum)
}

fn padding(properties: &BTreeMap<String, String>, fallback: egui::Vec2) -> [f32; 4] {
    let values = properties
        .get("padding")
        .map(String::as_str)
        .unwrap_or("")
        .split_whitespace()
        .filter_map(|value| value.trim_end_matches("px").parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map(|value| value.max(0.))
        .collect::<Vec<_>>();
    let mut values = match values.as_slice() {
        [all] => [*all; 4],
        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
        [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
        [top, right, bottom, left] => [*top, *right, *bottom, *left],
        _ => [fallback.y, fallback.x, fallback.y, fallback.x],
    };
    for (key, value) in [
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
    ]
    .into_iter()
    .zip(&mut values)
    {
        *value = number(properties, key, *value).max(0.);
    }
    values
}

fn font(ui: &egui::Ui, properties: &BTreeMap<String, String>) -> FontId {
    let family = if properties
        .get("font-family")
        .is_some_and(|value| value.contains("mono"))
    {
        egui::FontFamily::Monospace
    } else {
        ui.style()
            .override_font_id
            .as_ref()
            .map(|font| font.family.clone())
            .unwrap_or(egui::FontFamily::Proportional)
    };
    FontId::new(
        number(
            properties,
            "font-size",
            egui::TextStyle::Button.resolve(ui.style()).size,
        )
        .max(1.),
        family,
    )
}

fn stroke(properties: &BTreeMap<String, String>, fallback: Stroke) -> Stroke {
    let mut result = fallback;
    if let Some(border) = properties.get("border") {
        if border == "none" || border == "0" {
            result = Stroke::NONE;
        } else {
            for word in border.split_whitespace() {
                if let Some(color) = crate::ui::color(word) {
                    result.color = color;
                } else if let Ok(width) = word.trim_end_matches("px").parse::<f32>()
                    && width.is_finite()
                {
                    result.width = width.max(0.);
                }
            }
        }
    }
    result.width = number(properties, "border-width", result.width).max(0.);
    if let Some(color) = properties
        .get("border-color")
        .and_then(|v| crate::ui::color(v))
    {
        result.color = color;
    }
    result
}

fn radius(properties: &BTreeMap<String, String>, size: egui::Vec2, fallback: f32) -> f32 {
    properties
        .get("border-radius")
        .and_then(|value| {
            if let Some(value) = value.strip_suffix('%') {
                value
                    .parse::<f32>()
                    .ok()
                    .map(|value| size.min_elem() * value / 100.)
            } else {
                value.trim_end_matches("px").parse::<f32>().ok()
            }
        })
        .filter(|value| value.is_finite())
        .unwrap_or(fallback)
        .clamp(0., size.min_elem() * 0.5)
}

/// Visual size and pointer target size are independent. Compact typography does
/// not require an equally small pointer target; users may override both.
pub fn button(
    ui: &mut egui::Ui,
    id: &str,
    role: &str,
    classes: &[&str],
    label: &str,
    enabled: bool,
    selected: bool,
) -> Response {
    icon_button(ui, id, role, classes, label, enabled, selected, None)
}

pub type IconPainter<'a> = &'a dyn Fn(&egui::Painter, egui::Pos2, f32, Color32);

#[expect(
    clippy::too_many_arguments,
    reason = "native adapter explicitly receives semantic and interaction state"
)]
pub fn icon_button(
    ui: &mut egui::Ui,
    id: &str,
    role: &str,
    classes: &[&str],
    label: &str,
    enabled: bool,
    selected: bool,
    icon: Option<IconPainter<'_>>,
) -> Response {
    let state = if !enabled {
        "disabled"
    } else if selected {
        "selected"
    } else {
        "ready"
    };
    // Completion semantics provide a styling hook wherever an action is
    // rendered, including custom semantic layouts. The theme chooses its
    // dimensions and shape; the full engineering/action label remains intact.
    let mut action_classes = classes.to_vec();
    if matches!(role, "apply-action" | "cancel-action")
        && !action_classes.contains(&"confirmation-control")
    {
        action_classes.push("confirmation-control");
    }
    let classes = action_classes.as_slice();
    let props = properties(id, role, classes, state);
    let widget_id = ui.id().with(id);
    if props.get("display").is_some_and(|value| value == "none") {
        return ui.interact(egui::Rect::NOTHING, widget_id, egui::Sense::hover());
    }
    let font = font(ui, &props);
    let color = props
        .get("color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or_else(|| {
            if enabled {
                ui.visuals().text_color()
            } else {
                ui.visuals().weak_text_color()
            }
        });
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), font.clone(), color);
    let pad = padding(&props, ui.spacing().button_padding);
    let icon_size = icon
        .map(|_| number(&props, "icon-size", font.size * 1.45).max(0.))
        .unwrap_or(0.);
    let icon_gap = if icon.is_some() {
        number(&props, "gap", 4.).max(0.)
    } else {
        0.
    };
    let natural =
        galley.size() + egui::vec2(pad[1] + pad[3] + icon_size + icon_gap, pad[0] + pad[2]);
    let mut visual = egui::vec2(
        length(&props, "width", ui.available_width(), natural.x),
        length(
            &props,
            "height",
            ui.available_height(),
            natural.y.max(icon_size + pad[0] + pad[2]),
        ),
    );
    visual.x = visual
        .x
        .max(number(&props, "min-width", 0.))
        .min(length(&props, "max-width", ui.available_width(), f32::MAX))
        .max(0.);
    visual.y = visual
        .y
        .max(number(&props, "min-height", 0.))
        .min(length(
            &props,
            "max-height",
            ui.available_height(),
            f32::MAX,
        ))
        .max(0.);
    // Text overflow is an opt-in presentation choice. Keep the full source label
    // in semantic/accessibility data even when a narrow control elides its text.
    let galley = if props
        .get("text-overflow")
        .is_some_and(|value| value == "ellipsis")
    {
        let mut job = egui::text::LayoutJob::single_section(
            label.into(),
            egui::TextFormat {
                font_id: font,
                color,
                ..Default::default()
            },
        );
        job.wrap.max_width = (visual.x - pad[1] - pad[3] - icon_size - icon_gap).max(0.);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.break_on_newline = false;
        ui.painter().layout_job(job)
    } else {
        galley
    };
    let elided = galley.elided;
    let minimum = number(&props, "min-target-size", ui.spacing().interact_size.y).max(0.);
    let hit = egui::vec2(
        visual.x.max(number(&props, "hit-width", minimum)),
        visual.y.max(number(&props, "hit-height", minimum)),
    );
    let (reserved, _) = ui.allocate_exact_size(hit, egui::Sense::hover());
    if props
        .get("visibility")
        .is_some_and(|value| value == "hidden")
    {
        return ui.interact(reserved, widget_id, egui::Sense::hover());
    }
    let response = ui.interact(
        reserved,
        widget_id,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let pressed = enabled
        && (response.is_pointer_button_down_on()
            || (response.has_focus()
                && ui.input(|input| {
                    input.key_down(egui::Key::Enter) || input.key_down(egui::Key::Space)
                })));
    let interaction = format!(
        "{state}{}{}{}",
        if response.hovered() { " hover" } else { "" },
        if pressed { " active pressed" } else { "" },
        if response.has_focus() {
            " focus focused"
        } else {
            ""
        }
    );
    let actual = properties(id, role, classes, &interaction);
    let style = ui.style().interact(&response);
    let fill = actual
        .get("background")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or_else(|| {
            if selected {
                ui.visuals().selection.bg_fill
            } else {
                style.bg_fill
            }
        });
    let color = actual
        .get("color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or(color);
    let opacity = number(&actual, "opacity", 1.).clamp(0., 1.);
    let rect = egui::Rect::from_center_size(reserved.center(), visual);
    if !actual
        .get("visibility")
        .is_some_and(|value| value == "hidden")
    {
        let mut border = stroke(&actual, style.bg_stroke);
        border.color = border.color.gamma_multiply(opacity);
        ui.painter().rect(
            rect,
            radius(&actual, visual, style.corner_radius.nw as f32),
            fill.gamma_multiply(opacity),
            border,
            egui::StrokeKind::Inside,
        );
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
        let content_width = galley.size().x + icon_size + icon_gap;
        let left = match actual.get("text-align").map(String::as_str) {
            Some("left" | "start") => rect.left() + pad[3],
            Some("right" | "end") => rect.right() - pad[1] - content_width,
            _ => rect.center().x - content_width * 0.5,
        };
        if let Some(icon) = icon {
            icon(
                &painter,
                egui::pos2(left + icon_size * 0.5, rect.center().y),
                icon_size,
                color.gamma_multiply(opacity),
            );
        }
        painter.galley_with_override_text_color(
            egui::pos2(
                left + icon_size + icon_gap,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            color.gamma_multiply(opacity),
        );
    }
    if elided {
        response.on_hover_text(label)
    } else {
        response
    }
}

pub fn selectable(
    ui: &mut egui::Ui,
    id: &str,
    role: &str,
    label: &str,
    selected: bool,
) -> Response {
    button(ui, id, role, &["selectable"], label, true, selected)
}

/// Compact inspector grouping keeps the authored group identity, while exposing
/// independent heading and divider targets to the presentation resource.
pub struct InspectorSection<'a> {
    pub id: &'a str,
    pub role: &'a str,
    pub classes: &'a [&'a str],
    pub label: &'a str,
    pub state: &'a str,
    /// `None` keeps the content open; `Some` supplies the initial fold state.
    pub collapse: Option<bool>,
    /// A view may temporarily reveal a selected entity without replacing its ID.
    pub open: Option<bool>,
}

pub fn inspector_section(
    ui: &mut egui::Ui,
    section: InspectorSection<'_>,
    content: impl FnOnce(&mut egui::Ui),
) {
    let mut classes = section.classes.to_vec();
    if !classes.contains(&"inspector-section") {
        classes.push("inspector-section");
    }
    let props = properties(section.id, section.role, &classes, section.state);
    if !visible(&props) {
        return;
    }
    let target = STYLES.with(|snapshot| {
        let snapshot = snapshot.borrow();
        let parent = snapshot.target.clone().unwrap_or_else(|| {
            cad_core::presentation::Region {
                id: "workspace".into(),
                role: "workspace".into(),
                component: None,
                rect: [0.; 4],
                classes: vec![],
                resizable: false,
            }
            .style_target(&snapshot.layout, "ready")
        });
        if parent.node.id == section.id {
            let mut target = parent;
            target.node.role = section.role.into();
            for class in &classes {
                if !target.node.classes.iter().any(|existing| existing == class) {
                    target.node.classes.push((*class).into());
                }
            }
            target.node.states = section
                .state
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            target
        } else {
            parent.child(
                section.id,
                section.role,
                &classes
                    .iter()
                    .map(|class| (*class).into())
                    .collect::<Vec<_>>(),
                section.state,
            )
        }
    });
    with_target(target, ui, |ui| {
        ui.push_id(section.id, |ui| {
            ui.scope(|ui| {
                // Container paint belongs to this frame alone. In particular its
                // background, border and padding must not recolor every input field.
                apply_container_properties(ui, &props);
                frame_from_properties(ui, &props).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut folding = section.collapse.map(|default_open| {
                        let mut folding =
                            egui::collapsing_header::CollapsingState::load_with_default_open(
                                ui.ctx(),
                                ui.make_persistent_id(section.id),
                                default_open,
                            );
                        if let Some(open) = section.open {
                            folding.set_open(open);
                        }
                        folding
                    });
                    let heading = inspector_heading(ui, &section, folding.as_mut());
                    if let Some(folding) = folding.as_mut() {
                        folding.show_body_unindented(ui, content);
                    } else {
                        content(ui);
                    }
                    inspector_divider(ui, &section);
                    if let Some(heading) = heading
                        && heading.hovered()
                        && folding.is_some()
                    {
                        heading.on_hover_text("Click to expand or collapse this section");
                    }
                });
            });
        });
    });
}

fn visible(props: &BTreeMap<String, String>) -> bool {
    !props.get("display").is_some_and(|value| value == "none")
        && !props
            .get("visibility")
            .is_some_and(|value| value == "hidden")
}

fn inspector_heading(
    ui: &mut egui::Ui,
    section: &InspectorSection<'_>,
    mut folding: Option<&mut egui::collapsing_header::CollapsingState>,
) -> Option<Response> {
    let id = format!("{}/heading", section.id);
    let role = "inspector-section-heading";
    let expanded = folding.as_ref().is_none_or(|folding| folding.is_open());
    let state = format!(
        "{} {}",
        section.state,
        if expanded {
            "open expanded"
        } else {
            "closed collapsed"
        }
    );
    let props = properties(&id, role, &["inspector-heading"], &state);
    if !visible(&props) {
        return None;
    }
    let pad = padding(&props, egui::vec2(6., 3.));
    let font = font(ui, &props);
    let icon_size = if folding.is_some() {
        number(&props, "icon-size", font.size * 0.7).max(0.)
    } else {
        0.
    };
    let icon_gap = if folding.is_some() {
        number(&props, "gap", 5.).max(0.)
    } else {
        0.
    };
    let available = ui.available_width().max(0.);
    let width = length(&props, "width", available, available)
        .max(length(&props, "min-width", available, 0.))
        .min(length(&props, "max-width", available, available))
        .clamp(0., available);
    let mut text = egui::RichText::new(section.label).font(font);
    if props.get("font-weight").is_none_or(|value| {
        value == "bold" || value.parse::<u16>().is_ok_and(|weight| weight >= 600)
    }) {
        text = text.strong();
    }
    let text_width = (width - pad[1] - pad[3] - icon_size - icon_gap).max(0.);
    let wrap = if props
        .get("text-overflow")
        .is_some_and(|value| value == "ellipsis")
    {
        egui::TextWrapMode::Truncate
    } else {
        egui::TextWrapMode::Wrap
    };
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(wrap),
        text_width,
        egui::TextStyle::Button,
    );
    let elided = galley.elided;
    let natural_height = galley.size().y.max(icon_size) + pad[0] + pad[2];
    let height = number(&props, "height", natural_height)
        .max(number(&props, "min-height", 0.))
        .max(number(&props, "min-target-size", 0.))
        .min(number(&props, "max-height", f32::INFINITY))
        .max(0.);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let mut response = ui.interact(
        rect,
        ui.make_persistent_id(&id),
        if folding.is_some() {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let keyboard_activation = response.has_focus()
        && folding.is_some()
        && ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || input.consume_key(egui::Modifiers::NONE, egui::Key::Space)
        });
    if (response.clicked() || keyboard_activation)
        && let Some(folding) = folding.as_mut()
    {
        folding.toggle(ui);
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            if folding.is_some() {
                egui::WidgetType::CollapsingHeader
            } else {
                egui::WidgetType::Label
            },
            ui.is_enabled(),
            section.label,
        )
    });
    let state = format!(
        "{state}{}{}",
        if response.hovered() { " hover" } else { "" },
        if response.has_focus() {
            " focus focused"
        } else {
            ""
        }
    );
    let actual = properties(&id, role, &["inspector-heading"], &state);
    if visible(&actual) {
        let color = actual
            .get("color")
            .and_then(|value| crate::ui::color(value))
            .unwrap_or_else(|| ui.visuals().text_color());
        let opacity = number(&actual, "opacity", 1.).clamp(0., 1.);
        let fill = actual
            .get("background")
            .and_then(|value| crate::ui::color(value))
            .unwrap_or(Color32::TRANSPARENT)
            .gamma_multiply(opacity);
        let mut border = stroke(&actual, Stroke::NONE);
        border.color = border.color.gamma_multiply(opacity);
        ui.painter().rect(
            rect,
            radius(&actual, rect.size(), 0.),
            fill,
            border,
            egui::StrokeKind::Inside,
        );
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
        if let Some(folding) = folding.as_ref() {
            let center = egui::pos2(rect.left() + pad[3] + icon_size * 0.5, rect.center().y);
            let openness = folding.openness(ui.ctx());
            let right = [
                egui::vec2(-0.25, -0.4),
                egui::vec2(0.3, 0.),
                egui::vec2(-0.25, 0.4),
            ];
            let down = [
                egui::vec2(-0.4, -0.25),
                egui::vec2(0., 0.3),
                egui::vec2(0.4, -0.25),
            ];
            let points = right
                .into_iter()
                .zip(down)
                .map(|(right, down)| {
                    center + (right * (1. - openness) + down * openness) * icon_size
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(
                    number(&actual, "stroke-width", 1.4),
                    color.gamma_multiply(opacity),
                ),
            ));
        }
        let text_start = rect.left() + pad[3] + icon_size + icon_gap;
        let text_x = match actual.get("text-align").map(String::as_str) {
            Some("center") => text_start + (text_width - galley.size().x) * 0.5,
            Some("right" | "end") => rect.right() - pad[1] - galley.size().x,
            _ => text_start,
        };
        painter.galley_with_override_text_color(
            egui::pos2(text_x, rect.center().y - galley.size().y * 0.5),
            galley,
            color.gamma_multiply(opacity),
        );
    }
    Some(if elided {
        response.on_hover_text(section.label)
    } else {
        response
    })
}

fn inspector_divider(ui: &mut egui::Ui, section: &InspectorSection<'_>) {
    let props = properties(
        &format!("{}/divider", section.id),
        "inspector-section-divider",
        &["inspector-divider"],
        section.state,
    );
    if !visible(&props) {
        return;
    }
    let available = ui.available_width().max(0.);
    let width = length(&props, "width", available, available)
        .max(length(&props, "min-width", available, 0.))
        .min(length(&props, "max-width", available, available))
        .clamp(0., available);
    let thickness = number(&props, "stroke-width", 1.).max(0.);
    let height = number(&props, "height", 5.).max(thickness);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let color = props
        .get("color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or_else(|| ui.visuals().widgets.noninteractive.bg_stroke.color)
        .gamma_multiply(number(&props, "opacity", 1.).clamp(0., 1.));
    if thickness > 0. {
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            Stroke::new(thickness, color),
        );
    }
}

/// Secondary native groups share the inspector hierarchy and retain their stable
/// semantic/style identity. Folding never changes child commands or drafts.
pub fn collapsing(
    ui: &mut egui::Ui,
    id: &str,
    role: &str,
    label: &str,
    default_open: bool,
    content: impl FnOnce(&mut egui::Ui),
) {
    inspector_section(
        ui,
        InspectorSection {
            id,
            role,
            classes: &["secondary"],
            label,
            state: "ready",
            collapse: Some(default_open),
            open: None,
        },
        content,
    );
}

pub fn label(
    ui: &mut egui::Ui,
    id: &str,
    role: &str,
    classes: &[&str],
    text: &str,
    state: &str,
) -> Response {
    label_with_width_limit(ui, id, role, classes, text, state, None)
}

pub(crate) fn bounded_label(
    ui: &mut egui::Ui,
    id: &str,
    role: &str,
    classes: &[&str],
    text: &str,
    state: &str,
    width: f32,
) -> Response {
    // The caller has resolved this width against the complete field region.
    // Re-resolving percentages against the narrower caption would shrink them
    // a second time and move the unit/control away from their reserved column.
    label_with_width_limit(ui, id, role, classes, text, state, Some(width.max(0.)))
}

fn label_with_width_limit(
    ui: &mut egui::Ui,
    id: &str,
    role: &str,
    classes: &[&str],
    text: &str,
    state: &str,
    width_limit: Option<f32>,
) -> Response {
    let props = properties(id, role, classes, state);
    if props.get("display").is_some_and(|value| value == "none") {
        return ui.interact(egui::Rect::NOTHING, ui.id().with(id), egui::Sense::hover());
    }
    ui.push_id(id, |ui| {
        ui.scope(|ui| {
            apply_properties(ui, &props);
            let full_text = text;
            let mut text = egui::RichText::new(text).font(font(ui, &props));
            if props.get("font-weight").is_some_and(|value| {
                value == "bold" || value.parse::<u16>().is_ok_and(|weight| weight >= 600)
            }) {
                text = text.strong();
            }
            if let Some(color) = props.get("color").and_then(|value| crate::ui::color(value)) {
                text =
                    text.color(color.gamma_multiply(number(&props, "opacity", 1.).clamp(0., 1.)));
            }
            let elide = props.get("text-overflow").is_some_and(|v| v == "ellipsis");
            let widget = egui::Label::new(text).selectable(false);
            let widget = if elide { widget.truncate() } else { widget };
            if props
                .get("visibility")
                .is_some_and(|value| value == "hidden")
            {
                let size = egui::vec2(
                    width_limit
                        .unwrap_or_else(|| length(&props, "width", ui.available_width(), 0.)),
                    number(&props, "height", font(ui, &props).size),
                );
                return ui.allocate_response(size, egui::Sense::hover());
            }
            if width_limit.is_some() || props.contains_key("width") || props.contains_key("height")
            {
                let size = egui::vec2(
                    width_limit.unwrap_or_else(|| {
                        length(&props, "width", ui.available_width(), ui.available_width())
                    }),
                    number(&props, "height", font(ui, &props).size),
                );
                let response = ui.add_sized(size, widget);
                if elide {
                    response.on_hover_text(full_text)
                } else {
                    response
                }
            } else {
                let response = ui.add(widget);
                if elide {
                    response.on_hover_text(full_text)
                } else {
                    response
                }
            }
        })
        .inner
    })
    .inner
}

/// Menus use the same button renderer and keep a scoped style ancestry for all
/// menu entries. Native popup focus/closing behavior remains with egui.
pub fn menu_button<R>(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) {
    let response = button(ui, id, "menu", &["menu-heading"], label, true, false);
    let target = STYLES.with(|snapshot| {
        let snapshot = snapshot.borrow();
        snapshot
            .target
            .clone()
            .map(|parent| parent.child(id, "menu", &["menu-heading".into()], "ready"))
    });
    let content = |ui: &mut egui::Ui| {
        if let Some(target) = target {
            with_target(target, ui, content);
        } else {
            content(ui);
        }
    };
    if egui::containers::menu::is_in_menu(ui) {
        egui::containers::menu::SubMenu::new().show(ui, &response, content);
    } else {
        egui::Popup::menu(&response).show(content);
    }
}

pub fn expression(ui: &mut egui::Ui, id: &str, label: &str, value: &mut cad_core::Expr) -> bool {
    let props = properties(id, "expression-field", &["parameter-field"], "draft");
    if props.get("display").is_some_and(|value| value == "none") {
        return false;
    }
    self::label(
        ui,
        &format!("{id}.label"),
        "field-label",
        &[],
        label,
        "ready",
    );
    expression_edit(
        ui,
        id,
        label,
        value,
        false,
        "Value, units or parameter",
        None,
    )
    .changed()
}

/// Source-preserving expression editor without a second caption, also used by
/// dimension popovers whose focus and completion gestures need the response.
pub fn expression_edit(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    value: &mut cad_core::Expr,
    request_focus: bool,
    hint: &str,
    size: Option<egui::Vec2>,
) -> Response {
    let props = properties(id, "expression-field", &["parameter-field"], "draft");
    if props.get("display").is_some_and(|value| value == "none") {
        return ui.interact(egui::Rect::NOTHING, ui.id().with(id), egui::Sense::hover());
    }
    #[derive(Clone)]
    struct EditBuffer {
        source: String,
        text: String,
    }
    let edit_id = ui.id().with(id);
    let buffer_id = edit_id.with("source-buffer");
    let focused = request_focus || ui.memory(|memory| memory.has_focus(edit_id));
    let source = value.0.clone();
    let display = crate::quantity_display::expression(&source);
    let mut buffer = ui.data_mut(|data| {
        data.get_temp::<EditBuffer>(buffer_id)
            .unwrap_or_else(|| EditBuffer {
                source: source.clone(),
                text: if focused {
                    source.clone()
                } else {
                    display.clone()
                },
            })
    });
    if request_focus || !focused || buffer.source != source {
        buffer.text = if focused {
            source.clone()
        } else {
            display.clone()
        };
    }
    let response = ui
        .scope(|ui| {
            ui.style_mut().override_font_id = Some(font(ui, &props));
            if let Some(color) = props.get("color").and_then(|value| crate::ui::color(value)) {
                ui.visuals_mut().override_text_color = Some(color);
            }
            let pad = padding(&props, ui.spacing().button_padding);
            ui.spacing_mut().button_padding = egui::vec2(pad[1], pad[0]);
            let editor = egui::TextEdit::singleline(&mut buffer.text)
                .id(edit_id)
                .hint_text(hint)
                .desired_width(number(&props, "width", ui.available_width()))
                .font(font(ui, &props));
            if let Some(size) = size {
                ui.add_sized(size, editor)
            } else {
                ui.add(editor)
            }
        })
        .inner
        .on_hover_text(format!("{label}\nSource: {source}"));
    if request_focus {
        response.request_focus();
    }
    if response.changed() {
        value.0.clone_from(&buffer.text);
    } else if response.gained_focus() {
        buffer.text.clone_from(&source);
        if display != source {
            select_exact_source(ui, edit_id, &source);
        }
    } else if !response.has_focus() {
        buffer.text = display;
    }
    buffer.source.clone_from(&value.0);
    ui.data_mut(|data| data.insert_temp(buffer_id, buffer));
    response
}

pub(crate) fn select_exact_source(ui: &egui::Ui, id: egui::Id, source: &str) {
    if let Some(mut state) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(source.chars().count()),
            )));
        state.store(ui.ctx(), id);
    }
}

pub fn frame_from_properties(ui: &mut egui::Ui, props: &BTreeMap<String, String>) -> egui::Frame {
    let pad = padding(props, egui::vec2(4., 4.));
    let mut frame = egui::Frame::new().inner_margin(egui::Margin {
        top: pad[0].clamp(0., 127.) as i8,
        right: pad[1].clamp(0., 127.) as i8,
        bottom: pad[2].clamp(0., 127.) as i8,
        left: pad[3].clamp(0., 127.) as i8,
    });
    frame.fill = props
        .get("background")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or(Color32::TRANSPARENT);
    frame.stroke = stroke(props, Stroke::NONE);
    frame.corner_radius =
        egui::CornerRadius::same(radius(props, ui.available_size(), 0.).min(255.) as u8);
    frame
}

/// CSS container spacing and inherited typography do not change widget paint.
fn apply_container_properties(ui: &mut egui::Ui, props: &BTreeMap<String, String>) {
    ui.style_mut().override_font_id = Some(font(ui, props));
    if let Some(color) = props.get("color").and_then(|value| crate::ui::color(value)) {
        ui.visuals_mut().override_text_color = Some(color);
    }
    let gap = number(props, "gap", ui.spacing().item_spacing.x).max(0.);
    ui.spacing_mut().item_spacing = egui::vec2(
        number(props, "column-gap", gap).max(0.),
        number(props, "row-gap", gap).max(0.),
    );
}

/// Apply a resolved style to an adapter scope, including ordinary native fields.
pub fn apply_properties(ui: &mut egui::Ui, props: &BTreeMap<String, String>) {
    ui.style_mut().override_font_id = Some(font(ui, props));
    if let Some(color) = props.get("color").and_then(|value| crate::ui::color(value)) {
        ui.visuals_mut().override_text_color = Some(color);
    }
    let gap = number(props, "gap", ui.spacing().item_spacing.x).max(0.);
    ui.spacing_mut().item_spacing = egui::vec2(
        number(props, "column-gap", gap).max(0.),
        number(props, "row-gap", gap).max(0.),
    );
    let pad = padding(props, ui.spacing().button_padding);
    ui.spacing_mut().button_padding = egui::vec2(pad[1], pad[0]);
    let border = stroke(props, ui.visuals().widgets.inactive.bg_stroke);
    let fill = props
        .get("background")
        .and_then(|value| crate::ui::color(value));
    let radius = props
        .get("border-radius")
        .and_then(|value| value.trim_end_matches("px").parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map(|value| egui::CornerRadius::same(value.clamp(0., 255.) as u8));
    let visuals = &mut ui.visuals_mut().widgets;
    for visual in [
        &mut visuals.noninteractive,
        &mut visuals.inactive,
        &mut visuals.hovered,
        &mut visuals.active,
        &mut visuals.open,
    ] {
        visual.bg_stroke = border;
        if let Some(fill) = fill {
            visual.bg_fill = fill;
            visual.weak_bg_fill = fill;
        }
        if let Some(radius) = radius {
            visual.corner_radius = radius;
        }
    }
}

/// Viewport overlay anchors are resolved against the actual displayed viewport,
/// so duplicating or moving that viewport does not use a synthetic screen size.
pub fn overlay_rect(
    id: &str,
    role: &str,
    viewport: egui::Rect,
    default: egui::Rect,
) -> Option<egui::Rect> {
    overlay_rect_state(id, role, "ready", viewport, default)
}

pub fn overlay_rect_state(
    id: &str,
    role: &str,
    state: &str,
    viewport: egui::Rect,
    default: egui::Rect,
) -> Option<egui::Rect> {
    let props = properties(id, role, &["viewport-overlay"], state);
    if props.get("display").is_some_and(|value| value == "none")
        || props
            .get("visibility")
            .is_some_and(|value| value == "hidden")
    {
        return None;
    }
    let width = length(&props, "width", viewport.width(), default.width()).max(0.);
    let height = length(&props, "height", viewport.height(), default.height()).max(0.);
    let resolve = |key: &str, extent: f32| {
        props
            .get(key)
            .and_then(|value| {
                if let Some(value) = value.strip_suffix('%') {
                    value.parse::<f32>().ok().map(|value| extent * value / 100.)
                } else {
                    value.trim_end_matches("px").parse::<f32>().ok()
                }
            })
            .filter(|value| value.is_finite())
    };
    let x = resolve("left", viewport.width())
        .map(|value| viewport.left() + value)
        .or_else(|| {
            resolve("right", viewport.width()).map(|value| viewport.right() - width - value)
        })
        .unwrap_or(default.left());
    let y = resolve("top", viewport.height())
        .map(|value| viewport.top() + value)
        .or_else(|| {
            resolve("bottom", viewport.height()).map(|value| viewport.bottom() - height - value)
        })
        .unwrap_or(default.top());
    Some(egui::Rect::from_min_size(
        egui::pos2(x, y),
        egui::vec2(width, height),
    ))
}
