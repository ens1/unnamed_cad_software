# Nominal structural profile library

`nominal-sections.json` is TCAD's original, portable v1 structural-section catalog.
It includes 104 profiles in nine families: square/rectangular/round tube, angle,
channel, I beam, flat bar, solid round and solid square. Two collections offer
metric and imperial labels; all authoritative coordinates and radii are in
millimetres. Imperial dimensions use 25.4 mm per inch.

These are nominal design envelopes. Straight-sided shapes have sharp corners;
they do not include rolled root radii, flange taper or supplier tolerances.
Neither collection claims an ISO, EN, AISC, ASTM or supplier-certified section.
Engineering material and visual finish remain separate document definitions.
Original profile definitions carry TCAD's WTFPL license.

The application-owned contract is `cad_core::weldment_profiles`. Every catalog,
collection, profile and locator has a stable identity. Catalog and profile
revisions identify the source snapshot, and an `Origin` digest binds its complete
saved definition. Choosing a profile produces an ordinary pinned sketch of line
and circle members, with explicit `fix_x`/`fix_y` millimetre constraints. Named
locating points are fixed construction controls outside material contours.
The geometry kernel validates the resulting section during normal evaluation.

The first contour is the exterior. Further contours are disjoint holes strictly
inside it. Supported import geometry is a simple polygon or an analytic circle;
open paths, mixed material islands, arcs and splines need another contract
version. Catalog validation rejects nonfinite/oversized values, degenerate or
intersecting contours, touching/nested holes, missing collection identities and
duplicate profile/locator identities. Geometry validation has shared bounds:
4 MiB JSON, 2,000 profiles, 32,768 contour controls and a 4,000,000 pair budget.
Individual sections permit 17 contours, 1,024 controls and 64 locating points.

Import and export use the same JSON grammar. `parse_catalog` validates imports;
`catalog_json` validates before writing. Users can copy this file or start with a
small independent custom catalog, for example:

```json
{
  "version": 1,
  "id": "workshop-profiles",
  "revision": "1",
  "name": "Workshop profiles",
  "collections": [
    {
      "id": "custom-metric",
      "label": "My sections",
      "units": "metric",
      "notes": "Original nominal sections; coordinates are millimetres."
    }
  ],
  "profiles": [
    {
      "id": "custom-plate-30-5",
      "revision": "1",
      "collection": "custom-metric",
      "family": "custom",
      "label": "Support strip",
      "size": "30 × 5 mm",
      "notes": "Nominal rectangular support strip.",
      "contours": [
        { "kind": "polygon", "points": [[-15, -2.5], [15, -2.5], [15, 2.5], [-15, 2.5]] }
      ],
      "locators": [
        { "id": "center", "name": "Center", "point": [0, 0] },
        { "id": "bottom", "name": "Bottom center", "point": [0, -2.5] }
      ]
    }
  ]
}
```

Saved members retain their complete `Origin` even if the external catalog moves,
changes or disappears. Catalog reload never updates a committed section. A
section can be renamed without losing its catalog association; changing source
geometry or constraints requires an explicit conversion to a custom section or
a new profile selection. Source matching tolerates local identity remapping
when parts are inserted from libraries. Manufacturing geometry/size remains
the evaluated section's authority, rather than the catalog's descriptive label.
