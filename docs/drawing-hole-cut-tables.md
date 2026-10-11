# Associative drawing hole tables and weldment cut lists

The docked Drawings editor's Add Table menu includes Hole Table and Weldment Cut List. Both use the native drawing-table contract version 2 and the shared document draft. Their paper position, column widths, row height and text size remain editable. Apply waits for current validated geometry and commits the drawing transaction once; Cancel restores its checkpoint. Preview, drawing PDF and drawing DXF consume the same f64 table and annotation vectors.

## Hole table source and coordinates

A hole table selects one or more designed `Hole` or `DrilledHoles` features from one represented part and a specific assembly occurrence when necessary. Pathless sources require exactly one occurrence. A saved explicit configuration must match that occurrence's represented configuration; the table does not independently evaluate another sheet configuration.

The coordinate source is a saved Front, Top or Right projection and a native datum: the selected part occurrence's origin, an associative sketch point, an exact supported edge point, or a planar face origin. Hole axes must be normal to that view. Coordinates are signed model distances from the datum, expressed in millimetres or inches at precision 0–8, and do not change with paper scale. Removing the coordinate view is rejected while its table remains attached.

Rows use the configured legacy hole's saved origin/axis or current solved `DrilledHoles` center points, sketch plane, drilling direction and start offset. They report the actual bore or tap-drill diameter, blind depth or Through All intent, configured counterbore/countersink dimensions, and explicit cosmetic-thread diameter, pitch, handedness and depth where present. Centers and dimensions are never inferred from mesh holes. The entry coordinate describes the designed drill-axis station; it does not claim a physical trimmed entry rim on arbitrary stock.

Each table saves an editable alphanumeric/underscore tag prefix, and tables on the same view require different prefixes. The same prefix and reserved number appear in table rows and on-view leader tags; the datum receives a matching table-origin mark. Hole tags and datum marks use the same projection as that view. Automatic collision avoidance and tag dragging remain gaps.

Suppressed designed hole features omit their active rows while preserving source IDs, numbers and order. Missing or suppressed datum points, removed feature sources, missing current position geometry, ambiguous occurrences and oblique drill directions fail explicitly. Entirely suppressed/missing component occurrences cannot supply a resolved hole-table placement. Arbitrary imported bores, recognition from solid topology, patterned hole descendants without an explicit designed-position feature, grouping holes by equal size and arbitrary rotated table coordinate systems remain unsupported.

## Weldment cut-list source

A cut list selects the drawing's part/assembly scope or an included part, optionally filters represented configurations and native weldment output features, and updates from current evaluated native member metadata. It reports native member source, section, centerline length between the resolved end planes, start/finish cut angles, included assembled occurrence quantity and configured material. Exploding a drawing changes projection placements while manufacturing quantities remain assembled quantities.

Rows retain group/member/configuration identity. Suppressing a trim and restoring inherited stock keeps the member's item number even when its current output feature changes. Independent visible branches of the same member lineage are rejected instead of merging potentially different lengths; use a specific output or separate tables. Selected source outputs that no longer belong to the current frontier do not produce active rows. Non-native imported stock and legacy structural-member records are not promoted to evaluated native drawing cut-list rows.

Equivalent-stock grouping, longest physical blank dimensions, cap/gusset plate rows, profile dimensions, finish properties and automatic cut-list balloons remain gaps. An explicitly selected assembly configuration must match the current drawing scope; included part configurations must actually be represented. Nested assembly subscopes currently use the sheet's assembly scope or an included part rather than a separate nested assembly selector.

## Stable numbering and source identity

Item numbers are unique positive `u32` values with saved native source-key reservations. Model commands reconcile newly authored hole/member IDs within the same transaction, so adding source rows while the table editor is closed preserves existing numbers. Suppression, row reordering and configuration changes never recycle a reservation. Rows can be reordered independently of their numbers, and native semantic row IDs derive from the stable table/source identity. Review references guard removal of reserved row identities.

Default native member groups reserve every authored member ID, including stable legacy profile-edge and segment IDs accepted by the native path adapter. Explicit selections retain their saved member IDs. Reservation does not assert that a nonlinear or inactive path produces stock; current native member metadata determines active table rows. Source and active-row limits apply before collection.

Explicit-occurrence Make Unique retargets a hole table's feature ownership, relevant datum references and parsed reservation/order keys while retaining its numbers and review-row targets. Shared siblings and pathless definition-based tables retain their sources. Native member/control IDs remain native when feature ownership changes.

A table supports at most 10,000 active rows, 100,000 retained reservation/order entries and 8,192 bytes per source key. Number exhaustion and saved-data bounds produce explicit errors. Obsolete reservations remain until the user explicitly replaces/removes the table or edits its advanced native source; review references still guard retirement. Reusable vendor table formats, spreadsheet links, automatic pagination and standards-specific table libraries are not implemented.

Compilation and linting validate integration; interactive drawing acceptance remains with the user.
