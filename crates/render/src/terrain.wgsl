// Relief de la vue inclinée : maillage d'un morceau d'atlas texturé par ce morceau (et par celui du fond
// des transferts, en fondu), brouillard vers l'horizon ; ciel en dégradé derrière.

struct Frame {
    view_projection: mat4x4<f32>,
    // xyz : caméra en repère local ; w : pixels monde par mètre d'altitude.
    eye: vec4<f32>,
    fog_color: vec4<f32>,
    sky_color: vec4<f32>,
    // Début et fin du brouillard (distance au sol depuis la caméra), horizon en y normalisé, fondu.
    params: vec4<f32>,
    // Bordure et côté utile du morceau dans sa texture (part de la texture), altitude du point visé.
    uv: vec4<f32>,
};

struct Chunk {
    // Coin haut gauche du morceau en repère local, côté en pixels monde au zoom de la caméra.
    origin_size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var linear: sampler;
@group(1) @binding(0) var<uniform> chunk: Chunk;
@group(2) @binding(0) var main_texture: texture_2d<f32>;
@group(2) @binding(1) var transfer_texture: texture_2d<f32>;

struct TerrainOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) ground: vec2<f32>,
};

@vertex
fn terrain_vertex(@location(0) vertex: vec3<f32>) -> TerrainOut {
    let size = chunk.origin_size.z;
    let local = vec3<f32>(
        chunk.origin_size.x + vertex.x * size,
        chunk.origin_size.y - vertex.y * size,
        (vertex.z - frame.uv.z) * frame.eye.w,
    );
    var out: TerrainOut;
    out.position = frame.view_projection * vec4<f32>(local, 1.0);
    out.uv = frame.uv.x + vertex.xy * frame.uv.y;
    out.ground = local.xy;
    return out;
}

@fragment
fn terrain_fragment(in: TerrainOut) -> @location(0) vec4<f32> {
    let main = textureSample(main_texture, linear, in.uv);
    let transfer = textureSample(transfer_texture, linear, in.uv);
    let color = mix(main, transfer, frame.params.w);
    let distance = length(in.ground - frame.eye.xy);
    let fog = smoothstep(frame.params.x, frame.params.y, distance);
    return vec4<f32>(mix(color.rgb, frame.fog_color.rgb, fog), 1.0);
}

struct SkyOut {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc_y: f32,
};

// Un triangle qui couvre l'écran.
@vertex
fn sky_vertex(@builtin(vertex_index) index: u32) -> SkyOut {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    var out: SkyOut;
    out.position = vec4<f32>(x, y, 1.0, 1.0);
    out.ndc_y = y;
    return out;
}

@fragment
fn sky_fragment(in: SkyOut) -> @location(0) vec4<f32> {
    let horizon = frame.params.z;
    let t = clamp((in.ndc_y - horizon) / max(1.0 - horizon, 1e-3), 0.0, 1.0);
    return vec4<f32>(mix(frame.fog_color.rgb, frame.sky_color.rgb, sqrt(t)), 1.0);
}
