# Engineering materials and surface finishes

Engineering material determines density and manufacturing identification. Surface finish determines optical appearance and texture. TCAD stores and assigns them independently: painting, anodizing, or choosing a bare finish does not change a part's engineering density. New parts retain aluminum and clear anodized aluminum defaults unless the user explicitly changes the model's new-part preferences.

## Engineering catalog

The bundled [engineering catalog](../assets/materials/engineering.json) contains 60 sourced alloy, temper, condition, and stock-shape entries. It covers aluminum, carbon and alloy steel, stainless steel, titanium, copper, brass, bronze, nickel alloys, and engineering polymers. Practical sheet materials include 5052-H32 and 3003-H14; machining choices include 6061, 2024, 7075, C45, and 42CrMo4. Polymer records describe named supplier stock shapes rather than pretending that every formulation of a polymer has identical properties.

The library is separate from a project's material definitions. Adding a catalog entry creates a local definition with a fresh identity and its source snapshot. Existing parts and the four legacy project materials remain unchanged. A later catalog update does not change previously copied densities or properties.

Each record carries its family, grade, condition, tags, stable catalog origin, and citations. Published property values include their basis—typical, minimum, or maximum—and relevant product form, thickness, reference temperature, test method, and other conditions. Unknown properties remain absent. Ranges are not replaced by invented averages. A source's lower strength limit is labeled as a minimum for the stated condition, not a typical strength or a design allowable.

The selected source data come from [Hydro alloy sheets](https://www.hydro.com/us/us/aluminum/products/extruded-profiles/north-america-resources/north-america-alloy-data-sheets/), [Hulamin rolled-product datasheets](https://www.hulamin.com/aluminium-rolled-products/downloads), [Kaiser Aluminum technical data](https://online.kaiseraluminum.com/depot/PublicProductInformation/), [Outokumpu physical-property tables](https://www.outokumpu.com/-/media/from-sharepoint/documents/product/outokumpu-steel-grades-properties-global-standards.pdf), [Ovako Steel Navigator](https://steelnavigator.ovako.com/steel-grades/c45/), [SSAB hollow-section data](https://websitecdn.ssab.com/-/media/Files/EN/Support/Certificates/Hollow-Sections/Environmental-product-declaration---EN-EPD-SSAB-tubes-sections-piles.pdf), [Copper Development Association data](https://live.copper.org/publications/pub_list/pdf/a1360.pdf), [TIMET datasheets](https://www.timet.com/documents/datasheets/), [Special Metals technical bulletins](https://www.specialmetals.com/documents/technical-bulletins/inconel/inconel-alloy-625.pdf), [Ensinger stock-shape data](https://www.ensingerplastics.com/en/shapes/peek-tecapeek-natural), [Röchling HDPE data](https://www.roechling.com/fileadmin/assets/Technical%20Data%20Sheet%20Polystone%C2%AE%20G%20natural%20591262%20EN.pdf), and [Röhm acrylic data](https://www.plexiglas.de/files/plexiglas-content/pdf/technische-informationen/211-1-PLEXIGLAS-GS-XT-EN.pdf). Exact document links and retrieval dates are embedded in each copied record.

## Property and identity contracts

`cad_core::materials` owns engineering definitions and validation without renderer or kernel objects. Material metadata and optional properties are additive fields omitted from legacy definitions when absent. The original four material definitions and unchanged model settings retain their previous serialization.

| Quantity | Authoritative unit |
| --- | --- |
| Density | kg/m³ |
| Young's modulus, shear modulus, yield and tensile strength | Pa |
| Poisson's ratio, elongation | Dimensionless ratio |
| Thermal conductivity | W/(m·K) |
| Specific heat | J/(kg·K) |
| Linear thermal expansion | 1/K |
| Electrical resistivity | Ω·m |
| Melting and reference temperature | K |

Density and engineering properties use `f64`. Numeric local edits cannot retain an unchanged supplier attribution: editing a copied property marks the changed value as user-defined. Citations and catalog origin remain available as context. Explicit catalog replacement supplies its own provenance snapshot.

Assignments reference local material identities. Deletion is refused while a material is assigned to a part, referenced by a named configuration, or selected as the new-part default. A material used by a pinned component snapshot cannot be edited in place; make the component unique and duplicate the material first.

`ModelSettings.default_new_part_material` and `default_new_part_finish` affect subsequently created parts. They do not rescale geometry or change existing assignments. The unchanged engineering default is `aluminum`. In application Settings, the new-model default picker accepts a built-in, bundled catalog entry, or current-project material. Catalog/project choices save a complete portable snapshot with a fresh identity in personal preferences. New documents receive that snapshot before assignments are created; later edits to the source project or catalog do not silently change the saved default. Selecting a built-in clears the snapshot. The independent finish picker similarly retains a personal finish snapshot with its exact checksum-pinned texture files. New documents copy those assets into their own resource root; missing or altered assets prevent creating a document with invalid defaults. See [Settings](settings.md). Existing files retain their own definitions.

## Portable libraries

`cad_core::material_library::Catalog` uses schema version 1 with `name` and a list of material definitions. Parsing rejects unsupported versions, duplicate local identities, unsupported typed property fields, invalid physical values, missing property citations, oversized text, and catalogs exceeding 4 MiB or 2,000 entries. Parsing does not fetch citations or execute external content.

Import policies are explicit:

- **New identity** copies every entry to a fresh project identity and preserves its catalog origin.
- **Keep existing** skips matching local identities and imports the remaining records.
- **Replace existing** replaces matching local definitions subject to pinned-component guards.

Imports are planned and validated against a cloned document, then committed as one command. An invalid record cannot leave a partially imported library. Export includes the copied metadata and citation snapshot, allowing local catalogs to travel independently of the bundled library.

## Linked component snapshot integrity

New component links pin their exact imported definitions, engineering materials, and visual finishes. Their source JSON is saved as an immutable, hashed project resource. Updating or locating a component uses that exact JSON as evidence: source document, revision, canonical fingerprint, namespaced identities, materials, finishes, and imported resource records must agree before the document command is accepted. Changing a linked texture's resource record while leaving the finish unchanged is also rejected.

Ordinary Advanced Source edits cannot remove links, rewrite pin evidence, replace a pinned material or finish, or redirect a linked finish's texture. The materials and appearance editors expose pinned records as read only and retain duplication for local customization. Make the component unique before changing its assignments, then duplicate the material or finish to edit its properties.

Legacy links omit the new material/finish evidence fields. Opening a file enriches those fields only when the original saved snapshot bytes, source identity, fingerprint, imported definitions, and current material/finish/resource records can be verified. Loading does not rewrite files. A normal subsequent save persists verified enrichment. If evidence is missing, changed, or incompatible, the link keeps a visible diagnostic and its source assignments remain protected; no guessed hash or migration is substituted. The user can restore the original resource or perform an explicit verified source update. Runtime availability diagnostics do not alter authoritative document hashes.

Snapshot evidence is bounded to 64 MiB per link and 128 MiB per replacement command. Verified updates remain one ordinary undoable document operation; cancelling an operation retains committed definitions.

## Manufacturing tables and finish requirements

BOM and cut-list column selectors expose material family, grade, condition, density, and optional engineering properties. They derive these values from the configured part's local material snapshot. Engineering-property headings state their display units; value cells retain their typical, minimum, maximum, or user-defined basis. Configuration columns show authored names while source bindings retain stable configuration IDs.

A finish may also carry an explicitly authored manufacturing requirement: process, specification, designation, coating thickness in micrometres, specified surface Ra in micrometres, and notes. These fields remain separate from optical roughness and bump-map height. They are available as associative BOM/cut-list columns, along with informational finish category/description and authored face-finish names. A visual preset supplies no manufacturing requirement by default. Empty requirements stay blank; grouped occurrences with different requirements show `Multiple requirements` and can be split using definition/configuration/finish grouping.

Manufacturing requirements record design intent. They do not add coating solids, alter native surfaces, certify a process, or add coating mass to material-based mass calculations. A named display state can change viewport appearance without rewriting these manufacturing-table assignments.

## Data ownership and limits

The catalog contains a selected transcription of factual numeric values in an original TCAD arrangement. Original TCAD code and catalog arrangement use the repository's WTFPL license. Supplier publications, product names, and trademarks remain their owners'; their prose, page designs, PDFs, and logos are not bundled or relicensed. Citations identify the data source and do not imply supplier endorsement.

The catalog supports material selection, identification, and model mass calculations. It does not supply certified stock properties, fatigue curves, temperature-dependent datasets, anisotropic constitutive models, or simulation design allowables. The stated property conditions remain relevant even when a part's shape changes. Optical finishes and texture maps remain separate from engineering properties.
