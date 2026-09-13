struct PostUniforms {
    dimensions: vec2<f32>,
    exposure: f32,
    inverse_gamma: f32,
    bloom_strength: f32,
    bloom_threshold: f32,
    trail_decay: f32,
    vignette_strength: f32,
    chromatic_pixels: f32,
    tone_mapping: f32,
    padding: vec2<f32>,
};

@group(0) @binding(0) var scene_texture: texture_2d<f32>;
@group(0) @binding(1) var history_texture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> settings: PostUniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn fullscreen(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let position = positions[index];
    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = position * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return output;
}

fn pixel(uv: vec2<f32>) -> vec2<i32> {
    return clamp(vec2<i32>(uv * settings.dimensions), vec2<i32>(0), vec2<i32>(settings.dimensions) - vec2<i32>(1));
}

fn load_clamped(texture: texture_2d<f32>, coordinate: vec2<i32>) -> vec3<f32> {
    let maximum = vec2<i32>(settings.dimensions) - vec2<i32>(1);
    return textureLoad(texture, clamp(coordinate, vec2<i32>(0), maximum), 0).rgb;
}

@fragment
fn accumulate(input: VertexOutput) -> @location(0) vec4<f32> {
    let coordinate = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), vec2<i32>(settings.dimensions) - vec2<i32>(1));
    let scene = textureLoad(scene_texture, coordinate, 0);
    let history = textureLoad(history_texture, coordinate, 0);
    return vec4<f32>(scene.rgb + history.rgb * settings.trail_decay, max(scene.a, history.a * settings.trail_decay));
}

fn bloom_at(coordinate: vec2<i32>) -> vec3<f32> {
    let center = load_clamped(scene_texture, coordinate) * 4.0;
    let horizontal = load_clamped(scene_texture, coordinate + vec2<i32>(2, 0))
        + load_clamped(scene_texture, coordinate - vec2<i32>(2, 0));
    let vertical = load_clamped(scene_texture, coordinate + vec2<i32>(0, 2))
        + load_clamped(scene_texture, coordinate - vec2<i32>(0, 2));
    let blurred = (center + horizontal + vertical) / 8.0;
    return max(blurred - vec3<f32>(settings.bloom_threshold), vec3<f32>(0.0));
}

@fragment
fn finish(input: VertexOutput) -> @location(0) vec4<f32> {
    let coordinate = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), vec2<i32>(settings.dimensions) - vec2<i32>(1));
    let offset = vec2<i32>(i32(settings.chromatic_pixels), 0);
    let base = load_clamped(history_texture, coordinate);
    let alpha = textureLoad(history_texture, coordinate, 0).a;
    var color = vec3<f32>(
        load_clamped(history_texture, coordinate + offset).r,
        base.g,
        load_clamped(history_texture, coordinate - offset).b,
    );
    color += bloom_at(coordinate) * settings.bloom_strength;
    color *= settings.exposure;
    if settings.tone_mapping > 0.5 {
        color = color / (vec3<f32>(1.0) + color);
    }
    let centered = input.position.xy / settings.dimensions * 2.0 - vec2<f32>(1.0);
    let vignette = 1.0 - settings.vignette_strength * smoothstep(0.35, 1.25, dot(centered, centered));
    color *= vignette;
    color = pow(max(color, vec3<f32>(0.0)), vec3<f32>(settings.inverse_gamma));
    return vec4<f32>(color, alpha);
}
