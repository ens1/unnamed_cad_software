# Visual finishes and textures

TCAD stores surface appearance separately from engineering material. A part's engineering material still supplies its density, mass and manufacturing/BOM identity. Its visual finish supplies tint, roughness, metallic response, clearcoat and texture maps. Changing one does not change the other.

New parts use **Aluminum · Clear Anodized** (`clear-anodized-aluminum`). The unchanged four default finishes include bare aluminum, green anodized aluminum and painted RAL 1015. A separate lazy catalog offers 80 authored display presets, including machined, brushed, blasted and polished metals; dyed anodizing; matte, satin and gloss paint/powder-coat approximations; plastics, rubber, ceramic and wood approximations. Browsing the catalog does not insert unused finishes or alter a document fingerprint. The RAL preset is a display approximation, not a calibrated color specification. Aluminum presets use a bundled grayscale brushed-height image and directional reflections. Painted RAL 1015 defaults to no bump and isotropic reflections. The default finish may be edited or duplicated but cannot be deleted, because new parts depend on its stable identity.

## Editing a finish

Open **Appearance · Colors and Textures** from Tools, command search, or a
part/component's context actions. The bundled layouts provide a separate,
resizable Appearance panel beside the model viewport; the feature tree remains
available. Choose an explicit part and assignment scope, then assign a named
finish or customize that target. Target customization duplicates its finish and
assigns the copy in the common draft, so changing its color or texture does not
change unrelated parts that share the original. **Edit shared finish**
deliberately edits the library definition and every assignment using it.
Occurrence overrides and inheritance remain separate from the base part finish. Selecting a native face enables a face assignment scope; its optical finish takes precedence over the part/configuration/occurrence finish on that face only. Engineering material stays part-wide.

The panel includes tint, metallic, roughness, clearcoat, clearcoat roughness,
directional reflections, built-in bump selection, normal strength and bump depth. **Height / bump → Import/Replace** accepts a
user's grayscale image. Each texture slot also supports Clear and selection of
an existing pinned project image. Mapping controls set projection, physical tile
width/height, direction and U/V offsets. Complete finish JSON remains under
Advanced source. Older custom layouts without the dedicated panel retain the
property-manager editor; bind `workspace.appearance-panel` to place it anywhere.

Changes use the shared document draft and undo history. The viewport displays the proposed finish and lighting over the last valid geometry while the current evaluation is pending. **OK/Apply** requires that current validated evaluation, valid editor source and resolved renderer asset diagnostics; it commits one undoable change. **Cancel** restores the editor's starting checkpoint. Opening the editor and confirming without a source change closes it without adding an undo entry. Imported immutable files may remain as unreferenced project assets after Cancel; committed definitions stay unchanged.

## Grayscale relief and other maps

| Slot | Interpretation |
| --- | --- |
| Height / bump | Linear grayscale height, read from the red channel. White is higher than black. Bump depth is the black-to-white height range in millimetres. |
| Normal | Linear RGB tangent normals using the OpenGL **+Y** convention; normal strength controls this map's contribution. |
| Roughness | Linear red/grayscale values multiply the finish's roughness. |
| Metallic | Linear red/grayscale values multiply the finish's metallic value. |
| Occlusion | Linear red/grayscale values modulate diffuse and specular occlusion. |
| Color | sRGB RGB values multiply the finish's tint. |

Height gradients perturb the lighting normal, so relief responds to studio reflections and direct lights. A grayscale image can therefore produce the appearance of pores, machining marks or a raised pattern without encoding RGB normals. Bump depth defaults to **0.02 mm** and accepts **0–10 mm**. Normal and height contributions can be combined; normal strength does not scale the independently specified bump depth.

**The bump map drives grain.** With no imported height resource, **Built-in bump → Brushed grain** samples the original TCAD image at `assets/appearance/brushed-height-v1.png`: a seamless 1024×1024, 16-bit grayscale map containing irregular machining streaks along texture U. It follows the same floating-point height and mip pipeline as user images. There is no additional procedural shader grain layered over an import. An assigned height image replaces this map, including while that image is loading or has a reported error. Clear returns to the selected built-in map; selecting None with no imported height image removes bump relief. Tile sizes, texture direction and bump depth affect the actual grayscale map. The six original bundled 1024×1024, 16-bit grayscale maps cover brushed grain, blasted microtexture, machining marks, coating orange peel, molded texture and wood grain. Each preset chooses its physical tile size and relief depth. These are height pixels sampled through the imported-image pipeline, not hidden procedural shader grain. The bundled files can also be imported as regular project images; `scripts/generate-finish-maps.py` authors them deterministically under the repository WTFPL license. The original brushed file is unchanged.

Height gradients use a mip and finite-difference distance matched to the projected pixel footprint, then divide by that distance in physical millimetres. Fine relief filters away as the model recedes rather than retaining the full-resolution derivative at every zoom. **Directional reflections** (0–1) controls anisotropic specular response: 0 is isotropic, 1 is strongest. Its tangent follows the part-local texture U direction and remains independent of tessellation UVs. It defaults to 0.45 for clear/green anodized aluminum and 0.65 for bare aluminum. A separate imported roughness map can modulate surface response; the height map supplies surface relief.

Relief is a rendering effect. It does not move CAD surfaces, change silhouettes, create measurable geometry or alter volume, mass, tolerances, drawing geometry or STEP/STL output. There is no mesh displacement, parallax occlusion or volumetric texture contract in this implementation. Image alpha does not create surface transparency.

## Physical mapping and portable images

Texture coordinates use the part's local CAD millimetres, so they follow the part when its occurrence moves or rotates. **Triplanar** blends projections on the three local coordinate planes and works without mesh UVs. **Planar** uses the part's local XY plane. Both repeat the image at the requested U/V tile size, with direction and offsets applied before sampling. Scale accepts **0.001–1,000,000 mm** per axis. A finish shares one mapping across its maps. Individual face assignments can use separate finish definitions with their own map scale, direction and offsets. Per-map transforms, UV unwrapping and decals are not implemented.

Import accepts **PNG, JPEG, TIFF and WebP**. Files must be nonempty regular files, at most **64 MiB encoded**, at most **8192 pixels on either axis**, and at most **64 MiB of decoded RGBA pixels**. A 4096×4096 image fits the pixel limit. Decoder allocation is bounded to 128 MiB. Color, normal and ordinary scalar maps are converted to RGBA8. Height maps preserve 8/16-bit and normalized floating-point source values through floating-point mip generation, then use filterable single-channel R16Float GPU images. This retains more relief precision than RGBA8; the GPU half-float format still rounds source heights.

Images become checksum-pinned `Resource` entries rather than references to the original file's absolute path. Their bytes live under portable `imports/<sha256>.<extension>` paths. A finish refers to the resource ID. Import pins the exact bounded buffer checked for format and dimensions, without reopening the source. Changed, missing, unsupported or oversized files produce diagnostics; replacement imports pin the intended new bytes. Save As copies committed, draft and checkpoint resources before switching project roots. Component library imports namespace finish/resource IDs and retain exact resource bytes. Recovery snapshots record their resource root; manual recovery copies committed and pending resources into the recovered document's unsaved asset directory before replacing the active model. Missing recovery assets produce an error while preserving the active model.

The renderer generates a full mip chain, averages color in linear light, averages scalar/normal data as linear values, and uses repeat sampling, trilinear filtering and anisotropy. Import checks format and bounded dimensions before pinning bytes; full decoding and height-value validation complete in the asset worker before Apply. Image loading runs outside the UI thread with up to two concurrent loads. All finish maps share a repeat/linear sampler; obsolete images are released as document/root/finish sources change, and obsolete load replies are discarded. The runtime image cache is bounded to 128 images and 512 MiB of image data; cache exhaustion is reported rather than silently accepting an unavailable texture. Decoded assets are renderer caches, not CAD authority.

## Portable finish libraries and face assignments

Optional `Finish.manufacturing` carries explicit user-authored process, specification, designation, coating thickness (µm), surface roughness Ra (µm) and notes. Optical presets leave these requirements absent. These production fields are bounded source data, not measured surface results: neither bump depth nor a preset name infers thickness, roughness or certification. BOM/cut-list/drawing columns can expose the explicit requirements from base/configured model assignments; active viewport display-state overrides do not rewrite manufacturing requirements.

Catalog metadata labels the authored PBR parameters and sRGB swatches as display approximations. The common RAL names identify color families; they are not official licensed RAL numeric color data, calibrated manufacturing colors or coating process specifications. Presets do not populate strength, density or manufacturing values. Custom finishes retain editable properties and their source metadata; assignment and library editing remain separate actions.

Portable `.tcadfinishes` libraries carry versioned `tcad.surface-finishes` JSON, complete selected finish definitions and exact encoded texture bytes pinned by SHA-256. Export includes only referenced texture resources and rejects missing or changed files. Import verifies identities, bounds, portable paths, referenced assets and every byte checksum before installing a source clone. Resource identities are remapped to immutable project assets; conflicting finish identities receive fresh IDs rather than overwriting existing appearances. Apply also waits for the normal renderer image decoder/readiness validation. Bundled bump identifiers need no external file references.

A portable library supports 1–1024 finish definitions, at most 1024 texture records, at most 64 MiB of encoded texture bytes in total, and at most 256 MiB of serialized JSON. For larger collections, export smaller groups. Cancel keeps committed definitions unchanged; staged immutable images can remain unreferenced, as with ordinary texture import. Libraries contain visual source and assets only, without private part or assembly geometry.

Face assignments are part-owned and therefore shared by its occurrences, independently from component-wide overrides. They resolve against the current configured output's native faces. A face preserved by a downstream feature retains its finish; a removed face or nonunique descendant has no guessed substitute. Assignment diagnostics identify the retained source record and allow explicit removal or reattachment to the selected current face. A selected native face must be uniquely present in the current evaluation before it can be assigned. Component library links namespace part, face-assignment, producer, body, finish and texture identities; Make Unique remaps feature ownership while retaining the copied visual intent. Catalog/source metadata stays human-facing and is not remapped as document identity.

## Lighting

**Studio Lighting** exposes environment rotation, exposure in stops, environment intensity, key/fill intensity and shadows. These are saved scene settings and share the finish editor's draft/Apply/Cancel path. Exposure accepts **−8 to +8 stops**; intensity multipliers accept **0–10**.

The Bevy adapter generates a local studio environment with bright surrounding walls, five softbox reflection cards, cosine-filtered diffuse illumination and GGX-prefiltered specular mip levels. The environment supplies most metallic illumination; lower-intensity key/fill directional lights provide local highlights and cascaded key-light shadows. The studio uses 32-pixel diffuse faces and 128-pixel specular faces with eight roughness mip levels and 256 filter samples. A coarse studio is available at startup while full resources generate asynchronously. Cubemap handedness and environment rotation match direct-light directions. Disabling shadows also disables screen-space occlusion. Rendering uses an HDR camera, ACES fitted tone mapping, screen-space ambient occlusion and FXAA. Studio resources are generated locally; no network assets are needed. This is interactive rasterized PBR shading, not a ray-traced renderer. User HDR environment imports and a render/export workbench are not implemented. Sharp modeled edges remain sharp; textures do not substitute for geometric fillets or chamfers.

## Source and presentation contracts

The application-owned `tcad.appearance` contract is version **3**, defined in `cad-core::appearance`. It contains no Bevy materials, image handles or shader types. Visual numeric source values remain `f64`; the desktop adapter converts them at the rendering boundary.

- `Document.finishes` owns named `Finish` definitions; `Document.lighting` owns saved studio settings.
- `Part.face_finishes` stores optical overrides as stable `FaceFinish { id, body, face, finish }` source records. Unique current native construction references establish the target, using producer, topology and role plus body dependency ancestry; signatures, mesh order and proximity never reattach it. Missing, split or conflicting references retain recoverable assignments, publish warnings and show the part finish until explicitly repaired or removed. These warnings do not invalidate unrelated modeling.
- `Part.finish` chooses the base finish. `configuration::Overrides.finish` optionally chooses a configured part finish. `Instance.finish` optionally overrides a component occurrence.
- Nested occurrences inherit the nearest explicit ancestor override; a nearer override wins. With no occurrence override, the selected part configuration's finish wins over the base assignment. Patterned occurrences inherit their seed's appearance. Editing the base assignment preserves configured overrides.
- `TextureMaps` contains optional resource IDs for `base_color`, `normal`, `roughness`, `metallic`, `occlusion` and `height`. `TextureMapping` contains `mode`, `scale_mm`, `rotation_deg` and `offset_mm`.
- `Finish.builtin_bump` selects `none`, `brushed`, `blasted`, `machined`, `orange_peel`, `molded` or `wood` when no imported height resource is assigned. Optional `Finish.metadata` supplies category, search tags, description, source and catalog identity; it carries no engineering properties. `Finish.anisotropy` controls directional reflection strength. Version 2 reads older explicit finishes with these omitted fields defaulting to brushed and 0.45; existing stored colors, PBR values, texture resources and mappings remain intact. Previously texture-free legacy finishes therefore gain the built-in grain unless it is disabled.
- `PutFinish`, `DeleteFinish`, `SetPartFinish`, `SetFaceFinish`, `DeleteFaceFinish`, `SetInstanceFinish` and `SetLighting` use the common command transaction. Texture import adds its resource and finish reference together through that transaction.

Native schema 1 remains readable. Absent finish/lighting fields receive defaults. Unchanged default part assignments, default lighting, inherited instance assignments and the unchanged built-in library are omitted from serialization, preserving legacy source hashes and pinned part bytes. Library linking materializes default finish assignments before namespacing so an imported component retains its own visual definitions.

Semantic roles include `appearance-library`, `surface-finish`, `appearance-field`, `appearance-assignment`, `face-appearance-assignment`, `occurrence-appearance`, `scene-lighting` and `lighting-field`. Stable IDs include `finish:<id>::/roughness`, `finish:<id>::/textures/height`, `<part-id>::finish` and `viewport-lighting::/exposure`. Value descriptors publish choices, None/Inherit, units, bounds, editable states and disabled action reasons. Custom layouts bind these same fields through `appearance-finish`, `appearance-part`, `appearance-instance` and `appearance-lighting` setters; native and custom controls share commands and drafts.

CSS-like presentation resources can style, move, duplicate or hide these semantic controls. `workspace.appearance-panel` publishes open/closed state through the `appearance-panel` role; bundled themes reclaim its space when closed. Target selectors and customization actions are semantic controls with transient setters, separate from saved finish definitions. Styling a control does not change its finish or engineering material. The viewport adapter's lighting style overrides are separate presentation values; explicit overrides take precedence over saved scene settings without rewriting those settings. Supported viewport properties and their validation are published by the presentation contract. Selection continues to use localized outlines rather than replacing the finish; comparison and proposed geometry use their existing semantic overlay colors.

Compilation, formatting and linting are implementation checks. No automated tests, probes or interactive acceptance runs were performed; visual acceptance remains with the user.
