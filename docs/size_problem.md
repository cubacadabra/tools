the current morphs are too high-poly for a mobile-first first preview—but the download is not
  necessarily 10× Roblox’s total avatar download.

  Our current default starter downloads:

  - 7 packs
  - 8.30 MB
  - ~93,800 Near triangles
  - every pack contains Near/Mid/Far LOD data

  The largest contributors are:

  - Shoes: 1.94 MB / 20k triangles
  - Hoodie: 1.69 MB / 20k triangles
  - Base: 1.37 MB / 16k triangles
  - Hair: 1.26 MB / 16.9k triangles

  The textures are not the main problem; our generated atlases are only 128×128. Geometry, duplicated attributes, and
  shipping all three LODs are.

  For comparison, Roblox’s avatar body guidance uses a 10,742-triangle total body budget, and its accessory guidance
  lists 4k triangles per accessory. Our base alone exceeds the body budget, and several individual parts exceed the
  accessory guidance. Roblox body specifications (https://create.roblox.com/docs/avatar-setup/auto-setup-requirements),
  Roblox accessory specifications (https://create.roblox.com/docs/art/accessories/clothing-specifications)

  Roblox also streams assets as needed rather than requiring the entire avatar payload up front. Roblox asset streaming
  documentation (https://create.roblox.com/docs/reference/engine/classes/ContentProvider/AssetFetchFailed)

  My recommendation is to target roughly 2–4 MB for a complete first-preview avatar and 30k–50k Near triangles. We
  should reduce hidden clothing/shoe geometry, simplify hair/accessories, and eventually separate or compress LOD
  delivery. The concurrency/cache work helps repeat visits, but it cannot make the first 8.3 MB download feel fast.
