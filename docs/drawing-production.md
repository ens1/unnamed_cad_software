# Production tables and reports

TCAD derives BOMs and weldment cut lists from saved, configured definitions and current native fabrication metadata. A drawing table, its balloons, and its CSV export share the same source rows. Changing paper placement or column formatting does not change the component definitions or manufacturing dimensions.

## BOM structure and quantities

The drawing table editor offers **Parts only**, **Top level**, and **Indented hierarchy**. Parts only collects leaf parts throughout the assembly. Top level lists the root assembly's immediate parts and subassemblies. Indented hierarchy retains each subassembly and its children, grouping children within their matching parent definition/configuration chain.

Quantities count actual included occurrences in the requested model scope, including configured component patterns. Indented child quantities are totals across matching parent occurrences, rather than quantities per single parent. Every grouped row retains the full occurrence paths, selected configuration, parent paths, and definition-to-root f64 placements. The table's indentation is presentation of this hierarchy; saved item numbers do not flatten the children into a numerical sort.

Configured BOM exclusion on a part removes that part. Exclusion on a subassembly removes its branch. Suppressed components are absent from the configured traversal. Configuration-specific naming can use the document part number, the configuration name, or a required custom part number. Descriptions, material assignments, and custom part properties resolve in the same configuration.

The default grouping combines the same definition and configuration. The optional finish grouping separates occurrences with different visual finishes. When the default grouping combines several finishes, the Finish cell reads **Multiple finishes**. Engineering material and visual finish remain independent.

Choose, add, remove, or reorder columns in the table editor. BOM columns include item, part number, description, quantity, material, finish, configuration, definition, component type, level, and a named custom part property. Assembly rows currently have a definition/configuration name and configured BOM description, but no independent assembly material or arbitrary assembly property map.

## Weldment stock and accessories

Native weldment cut lists include structural members, end caps, and gusset regions from the selected output frontier. The table can cover the sheet's assembly/part scope or an included part, with explicit output and configuration filters.

**Individual source members** preserves each original section/path member identity while aggregating its represented occurrences. **Equivalent production stock** combines stock only when authored manufacturing intent and current evaluated production values prove a matching recipe. Every combined row retains each output, original member/accessory identity, configuration, and full occurrence path; a combined row never replaces those references with a fitted mesh shape.

Equivalent grouping compares solved authored section curves, section roll and locating point, engineering material, occurrence finish, evaluated centerline length, end angles, bend radius/sweep, accessory kind, thickness, blank dimensions, and the blank-bound representation. Geometry signatures use exact serialized f64 values, with no tolerance buckets or rounding to the displayed precision. Equivalent sections authored with a different curve orientation, segmentation, or locating convention can consequently remain separate.

For circular members, Length is the evaluated developed centerline length; radius and sweep have separate columns. Length is not a longest exterior blank dimension or a bend deduction. For caps and gussets, the stock width/height/thickness columns describe the native accessory stock rectangle. The length field on these accessories is the larger rectangle dimension. **Exact rectangle** and **Conservative control hull** distinguish actual analytic contour bounds from conservative positive-weight spline stock bounds. A conservative rectangle is never described as an exact contour extent.

Some equivalence cannot yet be proved from current metadata:

- Oblique end cuts and prioritized ButtTrim members retain their member identity because scalar end angles do not establish cut clocking on an asymmetric section.
- Caps on oblique ends retain their source identity for the same reason.
- Different gusset regions retain their region identities because equal blank rectangles do not establish equal outlines.

Those cases still produce source-linked cut-list rows and aggregate repeated occurrences of the same proved source. TCAD does not combine them solely because a mesh or bounding box looks similar.

The Individual source members mode rejects parallel carrier outputs for one original member lineage. Select one output frontier or choose Equivalent production stock to describe multiple explicitly retained output solids. This prevents different trimmed states from being counted as one unchanged source member.

Cut-list columns include the original eight production columns plus finish, configuration, definition, source, bend radius/sweep, accessory type, stock width/height, thickness, and stock-bound representation. Lengths display in millimeters or inches; cut and sweep angles display in degrees. Display precision does not participate in grouping.

## Item numbers, source bindings, and balloons

Tables save row-number reservations and source-key order. Suppressed or temporarily absent rows retain their reservations. New rows allocate numbers beyond existing reservations, so another member's suppression does not renumber a surviving item. The bounded reservation inventory is visible through the semantic drawing-table contract, including reserved rows that have no current geometry.

Cut-list balloons carry the table, row key, original member/accessory reference, and precise occurrence path. Auto cut-list balloons derive their labels and optional quantity from the current table, and their production position from native member or accessory metadata. A straight member uses its designed centerline midpoint, a circular member uses its actual path start station, and an accessory uses its native evaluated center. These are manufacturing reference stations; they are not fitted mesh centers or circular chord midpoints. Users can reattach the visible leader to a chosen drawing reference through the paper interaction tools.

Making one occurrence unique retargets its source-bound drawing references. Shared siblings keep their original definition, row reservations, and balloons. A row with surviving siblings gets a distinct reserved successor for the unique component; a fully retired active row transfers its number to its successor. Comments attached to retired rows follow the successor, while comments on a surviving shared row keep that row.

The source-key contracts are portable:

- Existing default parts-only BOM keys remain `definition@configuration`.
- Other BOM keys use canonical JSON with definition, configuration, optional finish grouping, and a flat parent-definition/configuration chain.
- Individual member keys retain the original configured member reference independently of later trim/cap carrier IDs.
- Equivalent-stock keys use a geometry digest plus explicit material, finish, and any conservative source identity. Ownership changes remap these explicit identities without altering the geometry digest.

Component library namespacing remaps these manifests, member references, occurrence paths, and synthetic circle/gusset region references while preserving literal labels and custom property text. Ambiguous legacy identities require explicit reattachment. A table restricted to one named part configuration cannot automatically express both shared and unique definitions after a split; TCAD retains that restriction and reports an affected balloon that needs table/source reattachment rather than silently broadening the configuration scope.

Changing grouping, material, finish, or production geometry can retire a group identity. A source-bound balloon whose original group is no longer represented requires reattachment; it does not silently adopt a row that happens to have the same item number.

## Preferences, paper pages, and exports

Production preferences are saved with the document's model settings. They select default BOM structure/grouping, stock grouping, report units, precision, and report columns. Newly created production tables copy these preferences. Each saved table remains individually editable.

Table splitting and pagination retain one source table and its complete row sequence. Continuation sections store their paper page and placement. Wrapped rows determine page breaks; TCAD reports a row that cannot fit rather than clipping or truncating it. The complete table's CSV output retains its columns, saved numbers, row order, displayed units, and all rows across its continuation pages.

Document report exports use the saved production preferences. Their item numbers are report-local enumeration; export the drawing table's CSV when the saved drawing item numbers must be retained. Existing `manufacturing::bom`, `csv`, `cut_list_csv`, and `cut_list_csv_evaluated` APIs preserve their original defaults and legacy headers. The configurable contracts are `bom::Options`, `cut_list::Options`, and `production::Preferences`; the configurable manufacturing wrappers and worker exports use those contracts.

## Validation and current boundaries

Manufacturing dimensions and stock grouping require the current complete native geometry evaluation. A drawing-only layout diagnostic can be repaired while this geometry cache remains complete; partial failed model evaluation is not manufacturing evidence. Source validation retains currently suppressed members and reserved pattern occurrences, and rejects removed source/type/ancestry identities. Current drawing generation also requires the bound source occurrence/configuration and member to be represented.

Enumeration is bounded: BOM traversal admits at most 100,000 occurrences, nesting at most 256 component levels, and saved JSON row identities at most 8,192 bytes. Stock grouping admits at most 100,000 source rows and member/occurrence bindings. Drawing tables retain at most 100,000 source-number reservations and 32 columns; their active row limit remains 10,000. Exceeding a bound produces a diagnostic rather than a complete-looking truncated report.

Deposited weld beads are visible geometry with their own production metadata, rather than independent purchased components. They do not create duplicate component BOM entries or structural-member cut-list rows. Stock nesting, longest exterior member blank calculations, optimized saw/cutting schedules, and independent multi-part configuration scopes within one restricted table remain future work.

Implementation verification follows repository policy: compilation, formatting, and linting only. Interactive acceptance and physical production review remain with the user.
