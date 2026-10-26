//! Original vector command symbols for the native presentation adapter.
//!
//! These are presentation hints, not command dispatch. Unknown commands retain
//! their complete accessible label and receive the same neutral action symbol.
use bevy_egui::egui::{self, Color32, Pos2, Stroke};

#[derive(Clone, Copy)]
enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy)]
enum Orientation {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Isometric,
}

#[derive(Clone, Copy)]
enum Symbol {
    Sketch,
    Line,
    Rectangle,
    Circle,
    Arc,
    Spline,
    Select,
    Extrude,
    Cut,
    Revolve,
    Loft,
    Sweep,
    Hole,
    Fillet,
    Chamfer,
    Shell,
    Pattern,
    Mirror,
    Plane,
    Axis,
    Dimension,
    Assembly,
    Mate,
    Move,
    Drawing,
    Save,
    Open,
    New,
    Undo,
    Redo,
    Settings,
    Appearance,
    Search,
    Fit,
    ZoomIn,
    ZoomOut,
    Check,
    Close,
    History,
    Model,
    Edit,
    Graph,
    Orbit(Direction),
    Pan(Direction),
    Upright,
    View(Orientation),
    Action,
}

fn symbol(command: &str) -> Symbol {
    let command = command.to_ascii_lowercase();
    let command = command
        .strip_prefix("command:")
        .or_else(|| command.strip_prefix("command-toolbar/"))
        .unwrap_or(&command);
    let tail = command.rsplit('.').next().unwrap_or(command);
    if command.starts_with("viewport.orientation.") {
        return match tail {
            "front" => Symbol::View(Orientation::Front),
            "back" => Symbol::View(Orientation::Back),
            "top" => Symbol::View(Orientation::Top),
            "bottom" => Symbol::View(Orientation::Bottom),
            "left" => Symbol::View(Orientation::Left),
            "right" => Symbol::View(Orientation::Right),
            "isometric" => Symbol::View(Orientation::Isometric),
            _ => Symbol::Action,
        };
    }
    match tail {
        "orbit-left" => Symbol::Orbit(Direction::Left),
        "orbit-right" => Symbol::Orbit(Direction::Right),
        "orbit-up" => Symbol::Orbit(Direction::Up),
        "orbit-down" => Symbol::Orbit(Direction::Down),
        "pan-left" => Symbol::Pan(Direction::Left),
        "pan-right" => Symbol::Pan(Direction::Right),
        "pan-up" => Symbol::Pan(Direction::Up),
        "pan-down" => Symbol::Pan(Direction::Down),
        "upright" | "normal-sketch" => Symbol::Upright,
        "model" if command.starts_with("presentation.navigation.") => Symbol::Model,
        "history" if command.starts_with("presentation.navigation.") => Symbol::History,
        "flow" if command.starts_with("presentation.navigation.") => Symbol::Graph,
        "arrange" if command.starts_with("graph.") => Symbol::Graph,
        "manager-edit" => Symbol::Edit,
        "apply" | "finish" | "confirm" => Symbol::Check,
        "cancel" | "close" => Symbol::Close,
        "undo" => Symbol::Undo,
        "redo" => Symbol::Redo,
        "save" | "save-as" | "publish" => Symbol::Save,
        "open" => Symbol::Open,
        "new-part" => Symbol::New,
        "new" if command.starts_with("file.") => Symbol::New,
        "settings" | "controls" => Symbol::Settings,
        "appearances" | "materials" | "lighting" => Symbol::Appearance,
        "commands" => Symbol::Search,
        "fit" | "fit-selection" | "fit-sketch" | "fit-support" | "fit-part-context" => Symbol::Fit,
        "zoom-in" => Symbol::ZoomIn,
        "zoom-out" => Symbol::ZoomOut,
        "latest" | "inspect" => Symbol::History,
        "select" => Symbol::Select,
        "line" | "polyline" | "centerline" | "midpoint-line" => Symbol::Line,
        "circle" | "perimeter-circle" | "ellipse" => Symbol::Circle,
        "smart-dimension" | "dimension" | "measure" => Symbol::Dimension,
        "mirror" | "mirror-body" | "mirror-feature" => Symbol::Mirror,
        "axis" | "face-axis" => Symbol::Axis,
        "plane" | "reference-plane" => Symbol::Plane,
        "shell" => Symbol::Shell,
        "fillet" => Symbol::Fillet,
        "chamfer" => Symbol::Chamfer,
        "move" | "rotate" | "transform" | "move-face" | "offset-face" => Symbol::Move,
        _ if tail.contains("rectangle") || tail == "parallelogram" => Symbol::Rectangle,
        _ if tail.contains("spline") || tail == "nurbs" => Symbol::Spline,
        _ if tail.contains("arc") || tail == "conic" || tail == "parabola" => Symbol::Arc,
        _ if tail.contains("pattern") => Symbol::Pattern,
        _ if tail.contains("revolve") => Symbol::Revolve,
        _ if tail.contains("loft") => Symbol::Loft,
        _ if tail.contains("sweep") => Symbol::Sweep,
        _ if tail.contains("hole") || tail.contains("bore") || tail.contains("sink") => {
            Symbol::Hole
        }
        _ if tail.contains("cut") || tail.contains("subtract") => Symbol::Cut,
        _ if tail.contains("extrude") || tail == "rib" => Symbol::Extrude,
        _ if command.starts_with("sketch.")
            || command.starts_with("sketch3d.")
            || tail == "sketch" =>
        {
            Symbol::Sketch
        }
        _ if command.contains("mate") => Symbol::Mate,
        _ if command.starts_with("assembly.") || tail == "assembly" || tail == "libraries" => {
            Symbol::Assembly
        }
        _ if command.starts_with("drawing") || tail == "bom" || tail == "manufacturing" => {
            Symbol::Drawing
        }
        _ if command.starts_with("surface.") => Symbol::Loft,
        _ => Symbol::Action,
    }
}

/// Draw into the supplied square; the caller owns sizing, style and command use.
pub fn paint(painter: &egui::Painter, center: Pos2, size: f32, color: Color32, command: &str) {
    if size <= 0. {
        return;
    }
    let scale = size / 24.;
    let at = |x: f32, y: f32| center + egui::vec2(x, y) * scale;
    let stroke = Stroke::new((1.5 * scale).max(0.8), color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    let path = |points: &[(f32, f32)], closed: bool| {
        let points = points.iter().map(|p| at(p.0, p.1)).collect();
        painter.add(if closed {
            egui::Shape::closed_line(points, stroke)
        } else {
            egui::Shape::line(points, stroke)
        });
    };
    let rect = |a: (f32, f32), b: (f32, f32)| {
        painter.rect_stroke(
            egui::Rect::from_min_max(at(a.0, a.1), at(b.0, b.1)),
            0.,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    let circle = |x, y, radius| {
        painter.circle_stroke(at(x, y), radius * scale, stroke);
    };
    let arrow = |a: (f32, f32), b: (f32, f32)| {
        let start = at(a.0, a.1);
        let end = at(b.0, b.1);
        painter.arrow(start, end - start, stroke);
    };
    let cube = || {
        path(
            &[
                (-9., -4.),
                (0., -9.),
                (9., -4.),
                (9., 6.),
                (0., 11.),
                (-9., 6.),
            ],
            true,
        );
        path(&[(-9., -4.), (0., 1.), (9., -4.)], false);
        line((0., 1.), (0., 11.));
    };
    let kind = symbol(command);
    match kind {
        Symbol::Sketch => {
            path(
                &[(-9., 6.), (5., -8.), (9., -4.), (-5., 10.), (-10., 11.)],
                true,
            );
            line((3., -6.), (7., -2.));
            line((-8., -9.), (-8., 0.));
            line((-8., -9.), (1., -9.));
        }
        Symbol::Line => {
            line((-8., 8.), (8., -8.));
            circle(-8., 8., 2.);
            circle(8., -8., 2.);
        }
        Symbol::Rectangle => rect((-10., -7.), (10., 7.)),
        Symbol::Circle => {
            circle(0., 0., 9.);
            line((-3., 0.), (3., 0.));
            line((0., -3.), (0., 3.));
        }
        Symbol::Arc => {
            let points = (0..=24)
                .map(|i| {
                    let angle = i as f32 / 24. * std::f32::consts::PI;
                    at(angle.cos() * 9., -angle.sin() * 9. + 4.)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            circle(-9., 4., 1.5);
            circle(9., 4., 1.5);
        }
        Symbol::Spline => {
            let points = (0..=32)
                .map(|i| {
                    let t = i as f32 / 32.;
                    at(-10. + 20. * t, -(t * std::f32::consts::TAU).sin() * 7.)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            circle(-10., 0., 1.5);
            circle(10., 0., 1.5);
        }
        Symbol::Select => path(
            &[
                (-7., -10.),
                (-6., 8.),
                (-1., 3.),
                (3., 10.),
                (6., 8.),
                (2., 1.),
                (9., 0.),
            ],
            true,
        ),
        Symbol::Extrude | Symbol::Cut => {
            cube();
            let down = matches!(kind, Symbol::Cut);
            if down {
                arrow((0., -11.), (0., -1.));
            } else {
                arrow((0., -1.), (0., -11.));
            }
        }
        Symbol::Revolve => {
            line((-3., -11.), (-3., 11.));
            path(&[(-3., -7.), (4., -7.), (4., 6.), (-3., 6.)], false);
            let points = (0..=24)
                .map(|i| {
                    let a = i as f32 / 24. * 5.;
                    at(a.cos() * 10., a.sin() * 4.)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            arrow((1., -4.), (5., -4.));
        }
        Symbol::Loft => {
            path(&[(-5., -9.), (5., -9.), (3., -6.), (-3., -6.)], true);
            path(&[(-10., 6.), (6., 4.), (10., 8.), (-6., 10.)], true);
            line((-5., -9.), (-10., 6.));
            line((5., -9.), (6., 4.));
            line((3., -6.), (10., 8.));
            line((-3., -6.), (-6., 10.));
        }
        Symbol::Sweep => {
            circle(-7., 6., 4.);
            path(
                &[(-7., 2.), (-7., -4.), (-4., -8.), (4., -9.), (9., -6.)],
                false,
            );
            path(
                &[(-3., 6.), (-3., -2.), (0., -4.), (5., -4.), (9., -1.)],
                false,
            );
            line((9., -6.), (9., -1.));
        }
        Symbol::Hole => {
            cube();
            circle(0., -4., 3.);
            line((-2., 3.), (-2., 7.));
            line((2., 3.), (2., 7.));
        }
        Symbol::Fillet => {
            path(
                &[
                    (-9., 10.),
                    (-9., -2.),
                    (-8., -5.),
                    (-5., -8.),
                    (-2., -9.),
                    (10., -9.),
                ],
                false,
            );
            path(
                &[(-5., 10.), (-5., 0.), (-4., -3.), (-1., -5.), (10., -5.)],
                false,
            );
        }
        Symbol::Chamfer => {
            path(&[(-9., 10.), (-9., -2.), (-2., -9.), (10., -9.)], false);
            path(&[(-5., 10.), (-5., 0.), (0., -5.), (10., -5.)], false);
            line((-9., -2.), (-5., 0.));
            line((-2., -9.), (0., -5.));
        }
        Symbol::Shell => {
            cube();
            path(&[(-5., -4.), (0., -7.), (5., -4.), (0., -1.)], true);
            line((-5., -4.), (-5., 3.));
            line((5., -4.), (5., 3.));
        }
        Symbol::Pattern => {
            for x in [-9., 3.] {
                for y in [-9., 3.] {
                    rect((x, y), (x + 6., y + 6.));
                }
            }
        }
        Symbol::Mirror => {
            line((0., -11.), (0., 11.));
            path(&[(-4., -7.), (-10., 7.), (-4., 7.)], true);
            path(&[(4., -7.), (10., 7.), (4., 7.)], true);
        }
        Symbol::Plane => path(&[(-10., 3.), (-3., -7.), (10., -3.), (3., 7.)], true),
        Symbol::Axis => {
            line((-10., 8.), (9., -9.));
            circle(0., 0., 5.);
            arrow((5., -5.), (10., -10.));
        }
        Symbol::Dimension => {
            line((-9., -7.), (-9., 10.));
            line((9., -7.), (9., 10.));
            line((-9., 4.), (9., 4.));
            arrow((-1., 4.), (-9., 4.));
            arrow((1., 4.), (9., 4.));
            line((-4., -6.), (4., -6.));
        }
        Symbol::Assembly => {
            rect((-10., -9.), (-1., 0.));
            rect((1., 1.), (10., 10.));
            path(&[(-6., 3.), (-6., 7.), (-2., 7.)], false);
            path(&[(2., -6.), (6., -6.), (6., -2.)], false);
        }
        Symbol::Mate => {
            circle(-4., -3., 6.);
            circle(4., 3., 6.);
            line((-3., 3.), (3., -3.));
        }
        Symbol::Move => {
            arrow((0., 0.), (0., -10.));
            arrow((0., 0.), (10., 0.));
            arrow((0., 0.), (-7., 7.));
        }
        Symbol::Drawing => {
            rect((-9., -11.), (9., 11.));
            rect((-5., -6.), (5., 3.));
            line((-6., 7.), (6., 7.));
            line((1., 7.), (1., 11.));
        }
        Symbol::Save => {
            path(
                &[(-9., -10.), (6., -10.), (10., -6.), (10., 10.), (-9., 10.)],
                true,
            );
            rect((-5., -10.), (4., -2.));
            rect((-5., 3.), (6., 10.));
        }
        Symbol::Open => {
            path(
                &[
                    (-10., 8.),
                    (-10., -7.),
                    (-2., -7.),
                    (1., -3.),
                    (9., -3.),
                    (9., 0.),
                ],
                false,
            );
            path(&[(-10., 8.), (-5., -1.), (11., -1.), (7., 8.)], true);
        }
        Symbol::New => {
            path(
                &[(-8., -10.), (3., -10.), (8., -5.), (8., 10.), (-8., 10.)],
                true,
            );
            path(&[(3., -10.), (3., -5.), (8., -5.)], false);
            line((-4., 3.), (4., 3.));
            line((0., -1.), (0., 7.));
        }
        Symbol::Undo | Symbol::Redo => {
            let sign = if matches!(kind, Symbol::Redo) {
                -1.
            } else {
                1.
            };
            path(
                &[
                    (-9. * sign, -2.),
                    (2. * sign, -2.),
                    (7. * sign, 1.),
                    (8. * sign, 5.),
                    (7. * sign, 8.),
                ],
                false,
            );
            path(
                &[(-3. * sign, -8.), (-9. * sign, -2.), (-3. * sign, 4.)],
                false,
            );
        }
        Symbol::Settings => {
            circle(0., 0., 7.);
            circle(0., 0., 2.5);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                line((a.cos() * 7., a.sin() * 7.), (a.cos() * 10., a.sin() * 10.));
            }
        }
        Symbol::Appearance => {
            circle(0., 0., 9.);
            path(&[(-5., 7.), (-7., 2.), (-5., -4.), (0., -8.)], false);
            circle(4., -3., 1.5);
            circle(5., 3., 1.5);
        }
        Symbol::Search | Symbol::ZoomIn | Symbol::ZoomOut => {
            circle(-2., -2., 7.);
            line((3., 3.), (10., 10.));
            if !matches!(kind, Symbol::Search) {
                line((-6., -2.), (2., -2.));
            }
            if matches!(kind, Symbol::ZoomIn) {
                line((-2., -6.), (-2., 2.));
            }
        }
        Symbol::Fit => {
            for (x, y) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
                path(
                    &[(x * 4., y * 10.), (x * 10., y * 10.), (x * 10., y * 4.)],
                    false,
                );
            }
            rect((-4., -4.), (4., 4.));
        }
        Symbol::Check => path(&[(-9., 0.), (-3., 7.), (10., -8.)], false),
        Symbol::Close => {
            line((-7., -7.), (7., 7.));
            line((-7., 7.), (7., -7.));
        }
        Symbol::History => {
            circle(0., 0., 9.);
            path(&[(0., -6.), (0., 0.), (5., 3.)], false);
        }
        Symbol::Model => {
            line((-8., -8.), (-8., 9.));
            for y in [-7., 1., 9.] {
                line((-8., y), (-2., y));
                rect((-2., y - 2.5), (8., y + 2.5));
            }
        }
        Symbol::Edit => {
            rect((-10., -10.), (3., 10.));
            line((-7., -6.), (0., -6.));
            line((-7., -2.), (-2., -2.));
            path(
                &[(-2., 8.), (7., -1.), (10., 2.), (1., 11.), (-3., 12.)],
                true,
            );
            line((5., 1.), (8., 4.));
        }
        Symbol::Graph => {
            rect((-10., -10.), (-3., -3.));
            rect((3., -3.), (10., 4.));
            rect((-10., 4.), (-3., 11.));
            path(&[(-3., -6.), (0., -6.), (0., 0.), (3., 0.)], false);
            path(&[(-3., 7.), (0., 7.), (0., 0.)], false);
        }
        Symbol::Orbit(direction) => {
            circle(0., 0., 4.);
            let transform = |(x, y): (f32, f32)| match direction {
                Direction::Right => (x, y),
                Direction::Left => (-x, y),
                Direction::Up => (y, -x),
                Direction::Down => (y, x),
            };
            let points = [
                (-9., -2.),
                (-10., 2.),
                (-7., 7.),
                (0., 9.),
                (7., 6.),
                (10., 0.),
            ]
            .into_iter()
            .map(transform)
            .collect::<Vec<_>>();
            path(&points, false);
            arrow(transform((7., 6.)), transform((10., 0.)));
        }
        Symbol::Pan(direction) => {
            rect((-4., -4.), (4., 4.));
            let delta = match direction {
                Direction::Left => (-1., 0.),
                Direction::Right => (1., 0.),
                Direction::Up => (0., -1.),
                Direction::Down => (0., 1.),
            };
            arrow((delta.0 * 5., delta.1 * 5.), (delta.0 * 11., delta.1 * 11.));
            line(
                (-delta.0 * 7. - delta.1 * 3., -delta.1 * 7. + delta.0 * 3.),
                (-delta.0 * 7. + delta.1 * 3., -delta.1 * 7. - delta.0 * 3.),
            );
        }
        Symbol::Upright => {
            line((-10., 9.), (10., 9.));
            arrow((0., 6.), (0., -10.));
            path(&[(-7., -3.), (-9., 0.), (-9., 5.)], false);
            path(&[(7., -3.), (9., 0.), (9., 5.)], false);
            line((-9., 5.), (-6., 2.));
            line((9., 5.), (6., 2.));
        }
        Symbol::View(orientation) => match orientation {
            Orientation::Isometric => cube(),
            Orientation::Front | Orientation::Back => {
                rect((-8., -8.), (8., 8.));
                let y = if matches!(orientation, Orientation::Front) {
                    5.
                } else {
                    -5.
                };
                line((-5., y), (5., y));
                let arrow_y = if matches!(orientation, Orientation::Front) {
                    11.
                } else {
                    -11.
                };
                arrow((0., arrow_y), (0., y));
            }
            Orientation::Top | Orientation::Bottom => {
                path(&[(-9., 2.), (-3., -7.), (10., -4.), (4., 5.)], true);
                let sign = if matches!(orientation, Orientation::Top) {
                    1.
                } else {
                    -1.
                };
                arrow((0., sign * -11.), (0., sign * -3.));
                line((-8., 9.), (8., 9.));
            }
            Orientation::Left | Orientation::Right => {
                path(&[(-5., -9.), (7., -4.), (7., 9.), (-5., 4.)], true);
                let sign = if matches!(orientation, Orientation::Right) {
                    1.
                } else {
                    -1.
                };
                arrow((sign * 11., 0.), (sign * 2., 0.));
            }
        },
        Symbol::Action => {
            path(&[(-8., 0.), (0., -8.), (8., 0.), (0., 8.)], true);
            painter.circle_filled(center, 2. * scale, color);
        }
    }
}
