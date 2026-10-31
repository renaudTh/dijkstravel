// Composition d'un morceau d'atlas : chaque calque est un rectangle de la cible, texturé (tuile
// d'imagerie retouchée, ombrage, frontières, niveau de mipmap précédent) ou uni (voile), posé par-dessus
// en couleurs prémultipliées.

struct Layer {
    // Gauche, haut, largeur, hauteur en pixels de la cible.
    rect: vec4<f32>,
    // Largeur et hauteur de la cible ; z : 0 texture telle quelle, 1 texture retouchée, 2 couleur unie.
    canvas: vec4<f32>,
    // Retouche de couleur, sur les couleurs non prémultipliées : matrix × rgba + offset.
    matrix: mat4x4<f32>,
    offset: vec4<f32>,
    // Couleur unie, prémultipliée.
    color: vec4<f32>,
};

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
