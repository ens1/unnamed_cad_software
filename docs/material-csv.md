# Material catalog CSV

The material browser imports `.csv` alongside `.tcadmaterials` and JSON. **Export
selected CSV** and **Export project CSV** write the same engineering records for
bulk editing in a spreadsheet or text editor. Import creates fresh local copies;
it does not overwrite assigned materials or linked snapshots. Apply and Cancel
use the existing material-library transaction.

The first record is a fixed 14-column header. Subsequent records contain one
material each, with the CSV version `1` and the same catalog name on every row.
Retain the header and its order. Density is a finite decimal number in **kg/m³**;
it does not accept implied unit conversions, localized decimal commas or formatted
thousands separators. Family uses the native enum spelling, such as `aluminum`,
`carbon_steel`, `stainless_steel` or `polymer`.

Ordinary columns expose ID, name, density, family, grade, condition and description.
`metadata_json` retains full source citations, tags, catalog origin and density
provenance. `properties_json` retains every optional SI engineering value and its
basis, conditions and provenance. The flat family/grade/condition/description
columns take precedence over those same fields inside metadata JSON when
importing an edited row. Empty metadata stays absent when those columns are also
empty. Missing engineering properties stay missing; no value is inferred from a
grade name. JSON cells must remain valid typed source.

For a CSV row whose original ID matches a current project record, changing density
or an engineering value while retaining its prior provenance makes that value
user-defined before the import creates a fresh local identity. The existing
record, including pinned records, is never edited by this normalization. Untouched
round-trips retain provenance; explicitly changed source assertions and unrelated
new catalog records remain authored claims.

Three `legacy_*` columns retain the old material color/metallic/roughness values
for source compatibility. Visual finishes and their texture resources remain
separate and use finish-library interchange.

CSV supports UTF-8 with an optional leading BOM, quoted cells, doubled quotes,
quoted embedded line breaks, and LF/CRLF/CR record endings. Invalid quotes,
duplicate identities, inconsistent catalog names, unsupported versions,
incorrect row widths and invalid material fields reject the complete import with
a row diagnostic. No URL is fetched and no spreadsheet expression is executed.

The input/output limit is 8 MiB and 2,000 material rows; the reconstructed native
catalog must also fit its existing 4 MiB JSON bound. File reads are bounded even
if the selected file grows. Empty catalogs use JSON because CSV rows carry the
catalog name. Export writes atomically. UI fields/actions remain semantic and
themeable. The CLI can export a project's catalog using
`tcad-cli materials PROJECT --output Materials.csv`; a JSON extension or omitted
output uses the existing native catalog format.

Compilation, formatting and strict linting are the implementation checks. No
tests, import probes or interactive runs are performed; user acceptance remains
pending.
