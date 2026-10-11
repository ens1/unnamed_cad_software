# Shell construction references

Shell publishes explicit application-owned construction references for its new faces and edges. Kernel topology IDs are temporary join keys during construction; they are not stored in the document. New roles use the existing `construction:` structured contract, containing normalized authored source references and constructor intent. Thickness, positions, centroids, tessellation and numerical signatures do not name the result.

Offset supporting walls and prismatic caps retain their original wall/cap reference as a source, with explicit inner or outer offset intent. The sketch builder returns member-to-edge constructor lineage for derived prism sections. A split source wall includes its original section edge and constructor part. Opening rings reference the original opening and contour edges. Supporting-plane rim partitions record their opening, clipping wall and preceding clipping closure.

Circular pocket reconstruction retains source stock ownership and records every expanded pocket face/edge from its original void closure and its factory cap, wall, station and edge part. The rebuilt opening wires are associated by actual shared constructor topology. A blind pocket whose expanded floor reaches the opposite inner stock wall is rejected because that requires intersecting-envelope trimming.

Original retained topology inherits its source role. Other shared edges reference their incident constructed walls. Repeated new edges with the same wall closure require explicit segmented ancestry and are reported as unsupported; TCAD does not guess their identity from position. Missing source provenance likewise fails explicitly.

These references remain stable while thickness and supported wall dimensions change without changing construction topology. Removing a contour, splitting or merging walls, changing the offset side, or changing the constructor domain may replace result identities. Native library namespacing and Make Unique traverse the structured source closure and preserve literal text/path fields.

The worker publishes the current validated result through the shared draft. Downstream sketch, datum and mate references resolve against the current owning body, while retaining their original producer provenance. Apply requires current validation and commits one undoable change; Cancel restores the editor checkpoint. No tests, probes or interactive acceptance exercises were used for this implementation.
