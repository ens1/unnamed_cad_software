# Finished dimensions and datums

Create a sketch, extrude it, and create a part drawing. TCAD reads the finished solid and places its measurable sizes on the sheet. The same sizes are available in **Tools → Inspect → Model Dimensions**. Pick a face or an edge to narrow the selection, or choose a finished dimension in the inspector. The model and its drawings share one tolerance for that dimension.

A 2-inch cube has three 2-inch overall dimensions. A 1-inch full-face cut leaves a 1-inch overall height. A 1-inch blind pocket leaves the original overall height and adds the depth between the pocket floor and its opening. Editing the cut changes those measurements. The extrude and its original sketch dimensions stay in the feature history.

## Tolerances

Set the general size and angle tolerances in **Settings → Application → New model defaults**. A new project takes a copy. Use **Settings → Model → Units and precision** to change them for the current project. Size input uses the selected length unit. Angle input uses degrees. The initial defaults are ±0.1 mm and ±0.5°. Manufacturing tolerances are independent of geometry and display accuracy.

In **Model Dimensions**, choose **Symmetric ±** or **Unequal limits** to override a dimension. Choose the default again to remove the override. Use Finish to commit the edit. Newly linked components retain the source project's general tolerances. A detached component can keep them or adopt the current document defaults.

## Datums and later edits

TCAD suggests planar faces and cylindrical axes as datum features. Select a suggestion to locate it in the part. Accept the suggested set in one action, accept individual letters, or assign a letter to another eligible face. Established datum symbols appear on part drawings and can supply letters to drawing tolerance frames.

Overall size follows the current part extents. Local sizes use the construction identities of their supporting faces. If a saved reference stops identifying a unique finished feature, **Needs review** names the affected requirement. Select a replacement for a datum. For a retired size, move its tolerance to a selected size with the same units or remove the override. Model editing can continue during review. Drawing export and publication identify unresolved requirements.

## Coverage and output

Recognition covers verifiable overall extents, parallel face spacings, prismatic pockets and steps, principal-axis hole locations, cylindrical diameters and lands, cylindrical fillet radii, measured chamfer setbacks, and conical sizes and included angles with surviving transverse rims. Oblique straight sides on a principal-plane profile can also supply corner coordinates. The model inspector lists omitted measurements when available evidence is insufficient. Freeform surfaces, interrupted features and complex intersections can need additional dimensioning.

A sheet stores callout placement. Moving a label changes its position on paper. **Edit in model** opens its source requirement. Use the sketch or feature editor to change the geometry that supplies a nominal size.

New part sheets and automatic PDF and drawing DXF exports include the model callouts by default. When callouts need more room, TCAD adds numbered continuation pages. Existing sheets can enable **Show model dimensions and datums**. The STEP geometry exporter does not currently write semantic PMI.

See [drawing creation and export](drawing-workspace.md) for the sheet controls.
