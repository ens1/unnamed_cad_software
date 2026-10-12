# Authored finish height maps

These original TCAD assets use the repository WTFPL license. They contain no
photographs, downloaded textures, licensed color samples or upstream design data.

`scripts/generate-finish-maps.py` deterministically authors seamless 1024 × 1024,
16-bit unsigned grayscale PNGs. Bright pixels are higher; dark pixels are lower.
The renderer differentiates the sampled grayscale height in physical millimetres
after mip filtering. No shader noise substitutes for these pixels.

| File | Texture |
| --- | --- |
| brushed-height-v1.png | Long interrupted machining/brush streaks along U; unchanged legacy default |
| blasted-height-v1.png | Isotropic multiscale microtexture |
| machined-height-v1.png | Fine directional interrupted tool marks |
| orange-peel-height-v1.png | Rounded coating microtexture |
| molded-height-v1.png | Molded multiscale texture |
| wood-height-v1.png | Directional irregular grain relief approximation |

The finish catalog supplies authored PBR values, physical tile dimensions and
relief depths. The maps are optical detail, not displacement, engineering
material data, measured roughness specifications or manufactured geometry.
Users can replace them with imported checksum-pinned grayscale image resources.
