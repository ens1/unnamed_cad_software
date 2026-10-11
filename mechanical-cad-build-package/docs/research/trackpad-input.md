# Trackpad input implementation notes

The product requirement is a primary trackpad workflow on macOS, Windows, and Linux. Gesture bindings in the main spec are an initial usability prototype, not a claim that identical native events are available everywhere.

## Platform integration

[Winit 0.30.13's WindowEvent documentation](https://docs.rs/winit/0.30.13/winit/event/enum.WindowEvent.html) documents native pinch and rotation events as available on macOS and iOS, its PanGesture event as iOS-only, and mouse-wheel events as including touchpad scrolling. Its smart-zoom event also has platform restrictions. Verify the actual pinned stack before selecting integration paths; the presence of a gesture event type does not establish desktop-platform parity.

Provide an application-owned semantic input layer. Normalize the available native gestures and scrolling events into orbit, pan, zoom, canvas navigation, and explicitly activated parameter adjustment. Preserve event precision and lifecycle information where available. Add platform adapters or documented trackpad equivalents when needed; do not make unsupported gestures prerequisites for a modeling operation.

## Gesture lifecycle

[Apple's Cocoa guide to handling trackpad events](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingTouchEvents/HandlingTouchEvents.html) distinguishes gesture phases from scroll momentum phases, describes cancellation and gesture interpretation changes, and notes that some system-wide gestures take precedence over application handling.

Use that information to keep an interaction's target stable, handle cancellation, suppress residual momentum before editing, and avoid conflicts with system gestures. Never interpret navigation momentum as continued parameter editing or commit a feature merely because a gesture ended.

## Product validation

Verify the complete workflow with physical hardware across supported desktop platforms, including built-in and external trackpads. Manually exercise fine adjustments, occluded selections, graph navigation, part isolation, and coordinated previews while geometry evaluation runs. The release criterion is comfortable and predictable CAD work with a trackpad and keyboard; mouse compatibility alone does not satisfy it. Do not write code tests or synthetic assertion suites.
