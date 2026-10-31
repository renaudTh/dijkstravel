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
    // Écran → repère local.
    inverse_view_projection: mat4x4<f32>,
    // xyz : vers le soleil ; w : part de l'éclairage (0 : couleurs de l'atlas telles quelles).
    sun: vec4<f32>,
    // rgb : lumière directe du soleil ; w : exagération du relief.
    sun_color: vec4<f32>,
    // rgb : lumière du ciel ; w : exposition.
    ambient: vec4<f32>,
    // Repère local → carte des ombres, vue orthographique depuis le soleil.
    light_view_projection: mat4x4<f32>,
    // x : côté d'un pixel de la carte des ombres en repère local ; y : 1 si elle est dessinée.
    shadow: vec4<f32>,
    // Largeur et hauteur de l'image, en pixels.
    screen: vec4<f32>,
    // xyz : vers le soleil, même couché.
    glow_direction: vec4<f32>,
    // rgb : lueur du soleil dans la brume et le ciel.
    glow_color: vec4<f32>,
    // x : nuit dans les couleurs (pâles et bleuies) ; y : étoiles.
    night: vec4<f32>,
};

struct Chunk {
    // Coin haut gauche du morceau en repère local, côté en pixels monde au zoom de la caméra.
    origin_size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var linear: sampler;
@group(0) @binding(2) var shadow_map: texture_depth_2d;
@group(0) @binding(3) var shadow_sampler: sampler_comparison;
@group(1) @binding(0) var<uniform> chunk: Chunk;
@group(2) @binding(0) var main_texture: texture_2d<f32>;
@group(2) @binding(1) var transfer_texture: texture_2d<f32>;

struct TerrainOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) ground: vec2<f32>,
    // Pente vers l'est et vers le nord, mètres par mètre.
    @location(2) slope: vec2<f32>,
    @location(3) local: vec3<f32>,
};

// Le sommet en repère local.
fn local_of(vertex: vec3<f32>) -> vec3<f32> {
    let size = chunk.origin_size.z;
    return vec3<f32>(
        chunk.origin_size.x + vertex.x * size,
        chunk.origin_size.y - vertex.y * size,
        (vertex.z - frame.uv.z) * frame.eye.w,
    );
}

// Le relief vu depuis le soleil : la carte des ombres.
@vertex
fn shadow_vertex(@location(0) vertex: vec3<f32>, @location(1) slope: vec2<f32>) -> @builtin(position) vec4<f32> {
    return frame.light_view_projection * vec4<f32>(local_of(vertex), 1.0);
}

@vertex
fn terrain_vertex(@location(0) vertex: vec3<f32>, @location(1) slope: vec2<f32>) -> TerrainOut {
    let local = local_of(vertex);
    var out: TerrainOut;
    out.position = frame.view_projection * vec4<f32>(local, 1.0);
    out.uv = frame.uv.x + vertex.xy * frame.uv.y;
    out.ground = local.xy;
    out.slope = slope;
    out.local = local;
    return out;
}

// Part de la lumière du soleil qui atteint le point : 0 dans l'ombre d'une crête. Moyenne de 3 × 3
// comparaisons filtrées : un bord d'ombre adouci. Hors de la carte, au soleil.
fn sunlit_share(local: vec3<f32>, normal: vec3<f32>) -> f32 {
    // Le point décollé de son versant d'un pixel et demi de la carte : contre l'acné des pentes rasantes.
    let lifted = local + normal * frame.shadow.x * 1.5;
    let clip = frame.light_view_projection * vec4<f32>(lifted, 1.0);
    let ndc = clip.xyz / clip.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let texel = 1.0 / f32(textureDimensions(shadow_map).x);
    var lit = 0.0;
    for (var dy = -1; dy <= 1; dy += 1) {
        for (var dx = -1; dx <= 1; dx += 1) {
            let offset = vec2<f32>(f32(dx), f32(dy)) * texel;
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, ndc.z);
        }
    }
    lit /= 9.0;
    // Vers le bord de la carte, l'ombre s'efface plutôt que de s'arrêter net.
    let edge = max(abs(ndc.x), abs(ndc.y));
    let inside = 1.0 - smoothstep(0.85, 1.0, edge);
    let outside = edge > 1.0 || ndc.z < 0.0 || ndc.z > 1.0 || frame.shadow.y < 0.5;
    return select(mix(1.0, lit, inside), 1.0, outside);
}

// La couleur de l'atlas éclairée par le soleil ou la lune (selon la pente, hors de l'ombre des crêtes) et
// par le ciel (selon l'ouverture). La nuit, elle pâlit et bleuit, comme vue par l'œil dans le noir.
fn sunlit(color: vec3<f32>, slope: vec2<f32>, local: vec3<f32>) -> vec3<f32> {
    let normal = normalize(vec3<f32>(-slope * frame.sun_color.w, 1.0));
    let facing = max(dot(normal, frame.sun.xyz), 0.0);
    let direct = frame.sun_color.rgb * facing * sunlit_share(local, normal);
    let sky = frame.ambient.rgb * (0.6 + 0.4 * normal.z);
    let lit = color * (direct + sky) * frame.ambient.w;
    let luma = dot(lit, vec3<f32>(0.2126, 0.7152, 0.0722));
    let night = mix(lit, luma * vec3<f32>(0.82, 0.92, 1.18), frame.night.x * 0.45);
    return mix(color, night, frame.sun.w);
}

// Lumière du soleil diffusée vers la caméra dans la direction `ray` : un halo autour de lui, qui
// s'attarde au-dessus de l'horizon après son coucher.
fn glow(ray: vec3<f32>) -> vec3<f32> {
    let facing = max(dot(normalize(ray), frame.glow_direction.xyz), 0.0);
    return frame.glow_color.rgb * (0.35 * pow(facing, 4.0) + 0.5 * pow(facing, 40.0)) * frame.sun.w;
}

// Nombre pseudo-aléatoire de 0 à 1 d'une case (hash12 de Dave Hoskins).
fn hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Étoiles fixes au ciel, dans la direction `ray` : une par case d'une grille en latitude et longitude
// célestes (cases de même aire), au plus ; `pixel` : angle d'un pixel, pour qu'elles ne scintillent pas en
// bougeant.
fn stars(ray: vec3<f32>, pixel: f32) -> vec3<f32> {
    let d = normalize(ray);
    let cell = 0.006;
    let latitude = asin(clamp(d.z, -1.0, 1.0));
    let row = floor(latitude / cell);
    let columns = max(floor(6.2831853 * cos((row + 0.5) * cell) / cell), 1.0);
    let longitude = atan2(d.y, d.x) + 3.14159265;
    let column = floor(longitude / 6.2831853 * columns);
    let seed = vec2<f32>(row, column);
    if hash(seed) > 0.06 {
        return vec3<f32>(0.0);
    }
    // L'étoile, quelque part dans sa case.
    let star_latitude = (row + 0.2 + 0.6 * hash(seed + 17.0)) * cell;
    let star_longitude = (column + 0.2 + 0.6 * hash(seed + 31.0)) / columns * 6.2831853 - 3.14159265;
    let star = vec3<f32>(cos(star_latitude) * cos(star_longitude), cos(star_latitude) * sin(star_longitude), sin(star_latitude));
    // Angle par la corde : précis pour les petits angles, là où acos ne l'est pas en 32 bits.
    let angle = length(d - star);
    let size = pixel * 0.8;
    let brightness = pow(hash(seed + 53.0), 5.0) * 1.6 + 0.3;
    let warmth = hash(seed + 71.0);
    let tint = mix(vec3<f32>(0.75, 0.85, 1.0), vec3<f32>(1.0, 0.92, 0.8), warmth);
    return tint * brightness * exp(-(angle * angle) / (size * size));
}

@fragment
fn terrain_fragment(in: TerrainOut) -> @location(0) vec4<f32> {
    let main = textureSample(main_texture, linear, in.uv);
    let transfer = textureSample(transfer_texture, linear, in.uv);
    let color = sunlit(mix(main, transfer, frame.params.w).rgb, in.slope, in.local);
    let distance = length(in.ground - frame.eye.xy);
    let fog = smoothstep(frame.params.x, frame.params.y, distance);
    // La brume s'allume vers le soleil.
    // La brume s'allume vers le soleil ; tout au fond, elle prend la couleur du ciel derrière le relief : il
    // s'y fond sans bord, là où il cesse d'être dessiné.
    let toward = vec3<f32>(in.ground - frame.eye.xy, 0.0);
    let ndc = vec2<f32>(in.position.x / frame.screen.x * 2.0 - 1.0, 1.0 - in.position.y / frame.screen.y * 2.0);
    let haze = mix(frame.fog_color.rgb + glow(toward), sky_at(ndc), smoothstep(0.5, 1.0, fog));
    return vec4<f32>(mix(color, haze, fog), 1.0);
}

struct SkyOut {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

// Un triangle qui couvre l'écran.
@vertex
fn sky_vertex(@builtin(vertex_index) index: u32) -> SkyOut {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    var out: SkyOut;
    out.position = vec4<f32>(x, y, 1.0, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}

// Le ciel, étoilé la nuit au-dessus de la brume de l'horizon : jamais sur le relief, qui s'y fond.
@fragment
fn sky_fragment(in: SkyOut) -> @location(0) vec4<f32> {
    let sky = sky_at(in.ndc);
    let horizon = frame.params.z;
    let t = clamp((in.ndc.y - horizon) / max(1.0 - horizon, 1e-3), 0.0, 1.0);
    if frame.night.y <= 0.0 || t <= 0.0 {
        return vec4<f32>(sky, 1.0);
    }
    // Direction du pixel et angle d'un pixel : entre ce rayon et celui du pixel au-dessus.
    let near = frame.inverse_view_projection * vec4<f32>(in.ndc, 0.0, 1.0);
    let far = frame.inverse_view_projection * vec4<f32>(in.ndc, 1.0, 1.0);
    let above = frame.inverse_view_projection * vec4<f32>(in.ndc + vec2<f32>(0.0, 2.0 / frame.screen.y), 1.0, 1.0);
    let ray = normalize(far.xyz / far.w - near.xyz / near.w);
    let next = normalize(above.xyz / above.w - near.xyz / near.w);
    let pixel = max(length(ray - next), 1e-5);
    let shine = stars(ray, pixel) * frame.night.y * smoothstep(0.0, 0.3, t) * frame.sun.w;
    return vec4<f32>(sky + shine, 1.0);
}

// Le ciel au point `ndc` de l'écran : la brume à l'horizon, le haut du ciel au-dessus, le halo du soleil.
fn sky_at(ndc: vec2<f32>) -> vec3<f32> {
    let horizon = frame.params.z;
    let t = clamp((ndc.y - horizon) / max(1.0 - horizon, 1e-3), 0.0, 1.0);
    // Direction du pixel : du plan proche au plan lointain.
    let near = frame.inverse_view_projection * vec4<f32>(ndc, 0.0, 1.0);
    let far = frame.inverse_view_projection * vec4<f32>(ndc, 1.0, 1.0);
    let ray = far.xyz / far.w - near.xyz / near.w;
    return mix(frame.fog_color.rgb + glow(ray), frame.sky_color.rgb + glow(ray) * 0.5, sqrt(t));
}
