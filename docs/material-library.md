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

`ModelSettings.default_new_part_material` and `default_new_part_finish` affect subsequently created parts. They do not rescale geometry or change existing assignments. The unchanged engineering default is `aluminum`.

## Portable libraries

`cad_core::material_library::Catalog` uses schema version 1 with `name` and a list of material definitions. Parsing rejects unsupported versions, duplicate local identities, unsupported typed property fields, invalid physical values, missing property citations, oversized text, and catalogs exceeding 4 MiB or 2,000 entries. Parsing does not fetch citations or execute external content.

Import policies are explicit:

- **New identity** copies every entry to a fresh project identity and preserves its catalog origin.
- **Keep existing** skips matching local identities and imports the remaining records.
- **Replace existing** replaces matching local definitions subject to pinned-component guards.

Imports are planned and validated against a cloned document, then committed as one command. An invalid record cannot leave a partially imported library. Export includes the copied metadata and citation snapshot, allowing local catalogs to travel independently of the bundled library.

## Data ownership and limits

The catalog contains a selected transcription of factual numeric values in an original TCAD arrangement. Original TCAD code and catalog arrangement use the repository's WTFPL license. Supplier publications, product names, and trademarks remain their owners'; their prose, page designs, PDFs, and logos are not bundled or relicensed. Citations identify the data source and do not imply supplier endorsement.

The catalog supports material selection, identification, and model mass calculations. It does not supply certified stock properties, fatigue curves, temperature-dependent datasets, anisotropic constitutive models, or simulation design allowables. The stated property conditions remain relevant even when a part's shape changes. Optical finishes and texture maps remain separate from engineering properties.
