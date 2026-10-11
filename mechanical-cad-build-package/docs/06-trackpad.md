# Trackpad input contract

Trackpad and keyboard are the primary tools for complete modeling, including sketching, selection, assemblies, editing, and precision work. A mouse or space mouse remains optional.

## Initial navigation proposal

| Surface | Gesture | Semantic action |
|---|---|---|
| Viewport | Primary click/tap | Select |
| Viewport | Two-finger movement | Orbit around a stable pivot |
| Viewport | Shift + two-finger movement | Pan |
| Viewport | Pinch, when exposed | Cursor-anchored zoom |
| Viewport | Configurable modifier + scroll | Trackpad zoom equivalent |
| Graph or planar sketch canvas | Two-finger movement | Pan the canvas |
| Graph/sketch | Supported pinch or equivalent | Zoom canvas |
| Rail lists | Two-finger scroll | Scroll content |
| Activated parameter/scale | Deliberate scrub gesture | Change draft parameter |
| Any surface | Fit selection / Fit assembly | Discoverable command and shortcut |

Offer a two-finger-pan profile with modifier orbit. The final default depends on physical usability sessions. Rotation/roll, smart zoom, pressure, haptics, and three/four-finger gestures are optional.

## Router responsibilities

Normalize native events into semantic actions before driving camera or document systems. Preserve high-resolution deltas, units, phases, cancellation, and momentum where supplied.

Capture the surface and interaction target at gesture begin and keep it until end/cancellation. A pointer crossing the rail border must not switch from camera movement to parameter scrubbing mid-gesture. Define explicit session behavior for platforms without reliable phase events.

Obtain surfaces and hit geometry from the current user layout; bind actions to semantic roles/identities instead of fixed left-panel coordinates or visual colors. Layout/theme reload preserves draft values and safely ends gesture captures whose presentation targets disappear.

Do not process the same physical gesture as both native gesture input and emulated wheel input. Stop/suppress navigation momentum when selecting or beginning edits. Parameter editing never inherits momentum.

A stable pivot must not change as a pointer crosses components. Large assemblies and small details need predictable camera movement and cursor-anchored zoom. Coordinate precision is independent of render-space precision.

## Precision and fatigue

Provide exact typed values, units, expressions, snapping, and a precision modifier. Add click-to-engage/move/click-to-finish interactions so long physical button holds are unnecessary.

Allow finger repositioning during an active adjustment without jumping values. Gesture end does not Apply. Lost focus and device changes terminate the relevant interaction safely and never silently commit.

## Platform evidence

The [existing input research](research/trackpad-input.md) examines Winit 0.30.13 and Apple gesture lifecycle documentation. It is background evidence, not proof that Bevy 0.20 exposes identical events on every desktop platform.

M0 must inspect the actual pinned windowing dependencies and capture event traces on hardware. Record device, OS/windowing environment, native event availability, precision, phase/momentum behavior, duplicate event handling, and usable fallbacks.

Build an application-owned platform adapter when necessary. Prefer supported native APIs; an event type's existence does not demonstrate Windows/macOS/Linux parity.

## First manual hardware sessions

1. Navigate a part while a slow preview evaluates.
2. Orbit and pan at both very small and assembly scales.
3. Select a narrow edge through overlapping components.
4. Scroll the rail without changing values.
5. Activate and scrub a radius, lift/reposition fingers, then Cancel.
6. Apply through the trackpad and through the keyboard.
7. Cancel a gesture, lose focus, and switch input device.
8. Edit an isolated shared part and peek its full assembly.

Record physical usability observations and event traces; do not write code tests or synthetic assertion suites. Cross-platform release claims require hardware evidence on supported platforms.
