//! Project the deposited side of a bead from its owner frame into a sheet view.
//! The occurrence and bead retain their own identities when the symbol is moved.
use super::*;
use crate::{
    PlacedPart,
    drawing::{CurveReference, PointReference, Units, WeldSymbol},
    weld_beads::{CalloutStation, Evaluated},
};

pub(super) struct Bead {
    symbols: [WeldSymbol; 2],
    stations: Vec<CalloutStation>,
    section_stations: Vec<CalloutStation>,
    tolerance: f64,
    visibility: BTreeMap<String, Vec<Option<bool>>>,
}

#[derive(Clone, Copy)]
pub(super) struct ProjectedSide {
    facing: f64,
    outward: Point,
}

impl ProjectedSide {
    fn arrow_side(self, leader: Point) -> bool {
        if self.facing.abs() > 1e-7 {
            self.facing > 0.
        } else {
            // Edge-on, the side the leader approaches on paper identifies the joint side.
            dot(leader, self.outward) >= 0.
        }
    }
}

struct Collector<'a> {
    doc: &'a Document,
    evaluation: &'a Evaluation,
    section: Option<&'a crate::drawing_sections::Evaluated>,
    items: &'a mut Vec<Item>,
    review: &'a mut Vec<String>,
}

#[derive(Default)]
pub(in crate::drawing) struct WeldItems {
    items: Vec<Item>,
    review: Vec<String>,
}
impl WeldItems {
    pub(in crate::drawing) fn collect(
        doc: &Document,
        e: &Evaluation,
        sheet: &Drawing,
        instances: &[DisplayInstance],
    ) -> Result<Self> {
        let mut result = Self::default();
        if !sheet.show_weld_symbols {
            return Ok(result);
        }
        let mut collector = Collector {
            doc,
            evaluation: e,
            section: e.drawing_sections.get(&sheet.id),
            items: &mut result.items,
            review: &mut result.review,
        };
        for displayed in instances {
            if displayed.effect.visibility == Some(crate::display_states::Visibility::Hidden) {
                continue;
            }
            let instance = &displayed.placement;
            let key = crate::history::mesh_key(instance);
            if let Some(weld) = e.assembly_welds.get(key) {
                // Assembly deposits are individual displayed bodies; the last path element
                // is the bead, while its owning assembly occurrence comes before it.
                let Some((last, owner_path)) = instance.path.split_last() else {
                    return Err(invalid("The assembly weld is missing its occurrence path"));
                };
                if last != &weld.bead.id || weld.owner != weld.bead.source {
                    return Err(invalid(
                        "The displayed weld has an inconsistent assembly source",
                    ));
                }
                collector.add(instance, owner_path, &weld.bead, &instance.name)?;
            } else if let Some(part) = doc.parts.get(&instance.definition) {
                for feature in &part.features {
                    if !matches!(feature.operation, crate::Operation::WeldBead { .. }) {
                        continue;
                    }
                    if let Some(bead) = e.weld_beads.get(&format!("{key}::{}", feature.id)) {
                        collector.add(instance, &instance.path, bead, &feature.name)?;
                    }
                }
            }
        }
        Ok(result)
    }

    pub(super) fn append(self, items: &mut Vec<Item>, review: &mut Vec<String>) {
        items.extend(self.items);
        review.extend(self.review);
    }
    pub(in crate::drawing) fn observe(
        &mut self,
        view: &str,
        scene: &mut crate::drawing_visibility::Projection,
    ) {
        // Check the center first. Large assemblies can exhaust this optional budget
        // without changing model visibility, the weld's identity or its physical side.
        let mut remaining = 250_000;
        for item in &mut self.items {
            if remaining == 0 {
                break;
            }
            let Style::Weld(bead) = &mut item.style else {
                continue;
            };
            let n = bead.stations.len();
            let mut hints = vec![None; n];
            for step in 0..n {
                if remaining == 0 {
                    break;
                }
                let i = (n / 2 + step) % n;
                hints[i] = scene.callout_visible(bead.stations[i].point, &mut remaining);
            }
            bead.visibility.insert(view.into(), hints);
        }
    }
}

impl Collector<'_> {
    fn add(
        &mut self,
        instance: &PlacedPart,
        path: &[Id],
        bead: &Evaluated,
        name: &str,
    ) -> Result<()> {
        if !bead.real_solid || bead.segment_count == 0 {
            return Ok(());
        }
        let Some(callout) = bead.callout.as_ref().filter(|c| !c.stations.is_empty()) else {
            self.review.push(format!(
                "{name}: rebuild the weld to locate its drawing symbol."
            ));
            return Ok(());
        };
        let units = match self.doc.settings.display_length_unit {
            crate::settings::LengthUnit::Inches | crate::settings::LengthUnit::Feet => {
                Units::Inches
            }
            _ => Units::Millimeters,
        };
        let symbols = [
            WeldSymbol::from_bead(callout, units, false)?,
            WeldSymbol::from_bead(callout, units, true)?,
        ];
        let m = instance.matrix;
        let stations: Vec<_> = callout
            .stations
            .iter()
            .map(|s| CalloutStation {
                point: transform(s.point, m),
                outward: world_direction(m, s.outward),
                tangent: world_direction(m, s.tangent),
            })
            .collect();
        let mut section_stations = Vec::new();
        // A cut can pass between sampled seam stations. Its actual intersection
        // with this weld is an authoritative arrow target on the section.
        if let Some(section) = self.section {
            for vertex in &section.vertices {
                let PointReference::SectionVertex { source, .. } = &vertex.reference else {
                    continue;
                };
                let CurveReference::ModelEdge {
                    edge,
                    body,
                    instance_path,
                } = source.as_ref()
                else {
                    continue;
                };
                if instance_path != &instance.path
                    || (body.as_ref() != Some(&bead.source) && edge.producer != bead.source)
                {
                    continue;
                }
                let distance = |s: &CalloutStation| {
                    (0..3)
                        .map(|i| (s.point[i] - vertex.position[i]).powi(2))
                        .sum::<f64>()
                };
                if let Some(closest) = stations
                    .iter()
                    .min_by(|a, b| distance(a).total_cmp(&distance(b)))
                {
                    section_stations.push(CalloutStation {
                        point: vertex.position,
                        ..*closest
                    });
                }
            }
        }
        let joint = callout.joint_type.label();
        let description = match callout.profile {
            crate::weld_beads::Profile::Fillet => "Fillet weld",
            crate::weld_beads::Profile::Butt if callout.square_groove => "Square-groove weld",
            crate::weld_beads::Profile::Butt => "Butt weld",
        };
        let text = if callout.joint_type == crate::weld_beads::JointType::Unspecified {
            description.to_owned()
        } else {
            format!("{joint} joint · {description}")
        };
        let id = serde_json::to_string(&(
            &instance.definition,
            &instance.configuration,
            path,
            "weld",
            &bead.id,
        ))?;
        self.items.push(Item {
            id,
            definition: instance.definition.clone(),
            source: bead.id.clone(),
            configuration: instance.configuration.clone(),
            path: path.to_vec(),
            label: name.into(),
            text,
            points: [stations[0].point; 2],
            vertex: None,
            normal: None,
            style: Style::Weld(Box::new(Bead {
                symbols,
                stations,
                section_stations,
                tolerance: self.evaluation.accuracy.modeling.linear_mm,
                visibility: BTreeMap::new(),
            })),
        });
        Ok(())
    }
}

impl Bead {
    fn in_view(&self, name: &str, view: &ViewProjection) -> Result<Option<Projection>> {
        let [x, y, w, h] = view.rect_mm;
        let mut best: Option<Projection> = None;
        // Mirroring a projection or placing the part rotates the deposited side
        // relative to the paper. The right-handed drawing basis points at the viewer.
        let project = |point| view.model_point(name, point);
        let zero = project([0.; 3])?;
        let [[xx, xy], [yx, yy], [zx, zy]] = [
            project([1., 0., 0.])?,
            project([0., 1., 0.])?,
            project([0., 0., 1.])?,
        ]
        .map(|v| diff(v, zero));
        let eye = nalgebra::Vector3::new(xx, yx, zx).cross(&nalgebra::Vector3::new(xy, yy, zy));
        for (index, station) in self
            .stations
            .iter()
            .chain(self.section_stations.iter().filter(|_| name == "section-z"))
            .enumerate()
        {
            if name == "section-z"
                && view
                    .section_frame
                    .is_none_or(|f| f.distance(station.point).abs() > self.tolerance)
            {
                continue;
            }
            let at = view.paper_point(name, station.point)?;
            if at[0] < x || at[0] > x + w || at[1] < y || at[1] > y + h {
                continue;
            }
            let tangent = diff(project(station.tangent)?, zero);
            let facing = eye
                .dot(&nalgebra::Vector3::from(station.outward))
                .clamp(-1., 1.);
            let visibility = self
                .visibility
                .get(name)
                .and_then(|v| v.get(index).copied().flatten());
            let clarity = match visibility {
                Some(false) => -4.,
                Some(true) => 0.2,
                None => 0.,
            };
            let middle = ((index as f64 + 0.5) / self.stations.len() as f64 - 0.5)
                .abs()
                .min(1.);
            let preference = match name {
                "isometric" => 0.08,
                "detail-front" | "section-z" => 0.09,
                _ => 0.,
            };
            let quality =
                0.8 * facing + 0.5 * length(tangent) + clarity - preference - 0.005 * middle;

            if best.as_ref().is_some_and(|p| p.quality >= quality) {
                continue;
            }
            best = Some(Projection {
                view: name.into(),
                anchor: at,
                points: [at, sum(at, tangent)],
                vertex: None,
                quality,
                true_size: false,
                weld_views: Vec::new(),
                weld_side: Some(ProjectedSide {
                    facing,
                    outward: diff(project(station.outward)?, zero),
                }),
            });
        }
        Ok(best)
    }

    pub(super) fn choose_view(
        &self,
        paper: &Sheet,
        saved: Option<&Placement>,
    ) -> Result<Option<Projection>> {
        let mut choices = Vec::new();
        for name in crate::drawing::VIEW_KEYS {
            if let Some(view) = paper.projections.get(name)
                && let Some(p) = self.in_view(name, view)?
            {
                choices.push(p);
            }
        }
        let views = choices.iter().map(|p| p.view.clone()).collect::<Vec<_>>();
        let best = choices.into_iter().max_by(|a, b| {
            let rank = |p: &Projection| saved.is_some_and(|s| s.view == p.view);
            rank(a)
                .cmp(&rank(b))
                .then_with(|| a.quality.total_cmp(&b.quality))
        });
        Ok(best.map(|mut p| {
            p.weld_views = views;
            p
        }))
    }

    pub(super) fn layout<'a>(
        &'a self,
        doc: &'a Document,
        definition: &'a str,
        p: &Projection,
    ) -> Result<Layout<'a>> {
        let mut bounds = [[[[0.; 2]; 2]; 2]; 2];
        for (side, symbol) in self.symbols.iter().enumerate() {
            for (hand, limit) in bounds[side].iter_mut().enumerate() {
                let body = symbol.body(doc, definition, [0.; 2], hand == 1)?;
                *limit = crate::drawing_interaction::bounds(&body)
                    .ok_or_else(|| invalid("Empty weld symbol"))?;
                limit[0] = limit[0].map(|x| x - 1.);
                limit[1] = limit[1].map(|x| x + 1.);
            }
        }
        Ok(Layout {
            bead: self,
            doc,
            definition,
            anchor: p.anchor,
            side: p
                .weld_side
                .ok_or_else(|| invalid("Missing projected weld side"))?,
            bounds,
        })
    }
}

pub(super) struct Layout<'a> {
    bead: &'a Bead,
    doc: &'a Document,
    definition: &'a str,
    anchor: Point,
    side: ProjectedSide,
    bounds: [[Bounds; 2]; 2],
}

impl Layout<'_> {
    fn orientation(&self, at: Point) -> (usize, usize) {
        let offset = diff(at, self.anchor);
        (
            usize::from(self.side.arrow_side(offset)),
            usize::from(offset[0] >= 0.),
        )
    }
    pub(super) fn bounds(&self, at: Point) -> Bounds {
        let (side, hand) = self.orientation(at);
        self.bounds[side][hand].map(|corner| sum(at, corner))
    }
    pub(super) fn place(
        &self,
        area: Bounds,
        occupied: &Occupancy,
        view_bounds: &BTreeMap<String, Bounds>,
    ) -> Option<Point> {
        let clear = |at: Point, exterior: bool| {
            let b = self.bounds(at);
            inside(b, area)
                && !occupied.intersects(b)
                && length(diff(at, self.anchor)) >= 4.
                && (!exterior
                    || !view_bounds
                        .values()
                        .any(|m| super::super::rectangles_overlap(b, *m)))
        };
        for exterior in [true, false] {
            for run in [7., 16., 28., 42.] {
                for rise in [8., -8., 17., -17., 30., -30.] {
                    for sign in [1., -1.] {
                        let at = sum(self.anchor, [sign * run, rise]);
                        if clear(at, exterior) {
                            return Some(at);
                        }
                    }
                }
            }
            // Search from both edges. A long tail can fit only on one hand.
            let dx = ((area[1][0] - area[0][0]) / 50.).max(4.);
            let dy = ((area[1][1] - area[0][1]) / 40.).max(4.);
            for row in 0..=((area[1][1] - area[0][1]) / dy) as usize {
                for col in 0..=((area[1][0] - area[0][0]) / dx) as usize {
                    let at = [area[0][0] + col as f64 * dx, area[1][1] - row as f64 * dy];
                    if clear(at, exterior) {
                        return Some(at);
                    }
                }
            }
        }
        None
    }

    pub(super) fn draw(
        &self,
        at: Point,
        out: &mut Vec<Primitive>,
        review: &mut Vec<String>,
        label: &str,
    ) -> Result<()> {
        let (side, hand) = self.orientation(at);
        let symbol = &self.bead.symbols[side];
        if length(diff(at, self.anchor)) < 3. {
            review.push(format!(
                "{label}: move the weld symbol at least 3 mm from its arrow point."
            ));
            out.extend(symbol.body(self.doc, self.definition, at, hand == 1)?);
        } else {
            symbol.render(
                self.doc,
                self.definition,
                self.anchor,
                diff(at, self.anchor),
                out,
            )?;
        }
        Ok(())
    }
}
