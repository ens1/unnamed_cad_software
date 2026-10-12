# Command palette

**Cmd/Ctrl+K** opens Find a Command in the shared workspace. Opening search
retains the current editor, source text, draft and checkpoint. **Esc** or **Close**
returns to that editor. Search itself does not apply, discard or rewrite model
definitions. Plain Enter invokes the selected current result; Up/Down changes
the result selection. Cmd/Ctrl+Enter does not apply a hidden editor while search
is open. The search field requests focus once when opened.

Search matches words in command names, IDs and categories. Available actions
sort first, followed by selected-geometry actions and named-view recall. The
optional availability filter hides disabled actions; otherwise disabled reasons
remain visible. At most 128 matches are displayed, with the complete match count
and a request to refine the query when needed.

The palette combines the registered command catalog with current material,
finish, lighting, property, hardware, design-monitor, tolerance-stack,
sketch-library, camera, paper and inspection actions. Selected-geometry actions carry their
actual targets. Named-view recall carries the saved view identity. Distinct
paper attachments sharing one command retain their own argument-bound results.
Named catalog entries, blocks, monitors, stacks and stack terms include the
target name in search results and retain their own source-bound actions.
Execution rechecks current document/draft identity, enabled state and argument
payload, restores the underlying editor, then delegates to the existing semantic
command route. It never bypasses the command's own current-source, selection,
kernel or Apply validation. Disabled controls are availability information, not
a claim that a kernel operation supports arbitrary geometry.

The presentation contract is `tcad.command-palette` version 1. Semantic fields
use the `command-palette` setter. `workspace.commands` exposes the registered
compound renderer; query, availability, status and individual result records
also have independent identities, roles, values and action data. User layouts
may replace the compound presentation with repeated semantic bindings and
CSS-like styling. Search state is transient presentation intent, outside CAD
source. Focus requests are limited to the requesting field's reserved focus
namespace and cannot remove modeling validation flags.

Compilation, formatting and strict linting are implementation checks. No tests
or interactive runs are performed; keyboard and hardware acceptance remain with
the user.
