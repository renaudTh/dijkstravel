// Composition d'un morceau d'atlas : chaque calque est un rectangle de la cible, texturé (tuile
// d'imagerie retouchée, ombrage, frontières, niveau de mipmap précédent) ou uni (voile), posé par-dessus
// en couleurs prémultipliées.

struct Layer {
    // Gauche, haut, largeur, hauteur en pixels de la cible.
    rect: vec4<f32>,
    // Largeur et hauteur de la cible ; z : 0 texture telle quelle, 1 texture retouchée, 2 couleur unie,
    // 3 ombrage.
    canvas: vec4<f32>,
    // Retouche de couleur, sur les couleurs non prémultipliées : matrix × rgba + offset.
    matrix: mat4x4<f32>,
    offset: vec4<f32>,
    // Couleur unie, prémultipliée.
    color: vec4<f32>,
    // Ombrage : intensité, force, ln de la base, base^(π/2) − 1 (`Hillshade::shading`).
    shading: vec4<f32>,
    // Ombrage : latitudes des bords haut et bas de la tuile de relief, en degrés.
    latitudes: vec4<f32>,
};

const PI: f32 = 3.14159265358979;
// Couleurs d'ombre et de lumière, direction de la lumière (335°, plus π) : celles de `hillshade.rs`.
const SHADOW: vec3<f32> = vec3<f32>(31.0, 42.0, 26.0) / 255.0;
const HIGHLIGHT: vec3<f32> = vec3<f32>(255.0, 247.0, 224.0) / 255.0;
const AZIMUTH: f32 = 335.0 * PI / 180.0 + PI;
// Côté de la grille des pentes : la tuile de relief et une bordure d'un texel.
const PREPARED: f32 = 258.0;

// `standard_hillshade` de MapLibre, en couleur prémultipliée.
fn hillshade(uv: vec2<f32>) -> vec4<f32> {
    // Texel k de la grille : pixel k − 1 de la tuile.
    let slopes = textureSample(source, bilinear, (uv * (PREPARED - 2.0) + 1.0) / PREPARED).rg;
    let latitude = mix(layer.latitudes.y, layer.latitudes.x, 1.0 - uv.y);
    let deriv = (slopes - 128.0 / 255.0) * (8.0 / cos(radians(latitude)));
    let slope = atan(0.625 * length(deriv));
    var aspect = PI / 2.0 * select(-1.0, 1.0, deriv.y > 0.0);
    if deriv.x != 0.0 {
        aspect = atan2(deriv.y, -deriv.x);
    }
    let intensity = layer.shading.x;
    let strength = layer.shading.y;
    var scaled = slope;
    if intensity != 0.5 {
        scaled = (exp(slope * layer.shading.z) - 1.0) / layer.shading.w * PI / 2.0;
    }
    let accent = (1.0 - cos(scaled)) * strength;
    let turn = (aspect + AZIMUTH) / PI + 0.5;
    let shade = abs(turn - 2.0 * floor(turn / 2.0) - 1.0);
    let weight = sin(scaled) * strength;
    let rgb = mix(SHADOW, HIGHLIGHT, shade) * weight;
    return clamp(vec4<f32>(rgb, accent * (1.0 - weight) + weight), vec4<f32>(0.0), vec4<f32>(1.0));
}

@group(0) @binding(0) var<uniform> layer: Layer;
@group(0) @binding(1) var bilinear: sampler;
@group(1) @binding(0) var source: texture_2d<f32>;

struct LayerOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Quatre sommets en bande : les coins du rectangle.
@vertex
fn layer_vertex(@builtin(vertex_index) index: u32) -> LayerOut {
    let corner = vec2<f32>(f32(index & 1u), f32((index >> 1u) & 1u));
    let pixel = layer.rect.xy + corner * layer.rect.zw;
    var out: LayerOut;
    out.position = vec4<f32>(pixel.x / layer.canvas.x * 2.0 - 1.0, 1.0 - pixel.y / layer.canvas.y * 2.0, 0.0, 1.0);
    out.uv = corner;
    return out;
}

@fragment
fn layer_fragment(in: LayerOut) -> @location(0) vec4<f32> {
    let mode = layer.canvas.z;
    if mode > 2.5 {
        return hillshade(in.uv);
    }
    if mode > 1.5 {
        return layer.color;
    }
    let texel = textureSample(source, bilinear, in.uv);
    if mode < 0.5 {
        return texel;
    }
    let straight = select(vec3<f32>(0.0), texel.rgb / texel.a, texel.a > 0.0);
    let adjusted = clamp(layer.matrix * vec4<f32>(straight, texel.a) + layer.offset, vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(adjusted.rgb * adjusted.a, adjusted.a);
}
