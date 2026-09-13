struct Particle {
    position_age: vec4<f32>,
    velocity_lifetime: vec4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
};

struct VolumeUniforms {
    grid_size: u32,
    particle_count: u32,
    ray_steps: u32,
    pixel_scale: u32,
    time: f32,
    density_scale: f32,
    absorption: f32,
    emission: f32,
    dimensions: vec2<f32>,
    motion_scale: f32,
    padding: u32,
};

@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read_write> density: array<atomic<u32>>;
@group(0) @binding(2) var<uniform> settings: VolumeUniforms;

fn hash(value: u32) -> f32 {
    var x = value;
    x = ((x >> 16u) ^ x) * 0x45d9f3bu;
    x = ((x >> 16u) ^ x) * 0x45d9f3bu;
    return f32((x >> 16u) ^ x) / 65535.0;
}

fn item_index(id: vec3<u32>) -> u32 {
    return id.x + id.y * 65535u * 256u;
}

@compute @workgroup_size(256)
fn clear_density(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = item_index(id);
    let count = settings.grid_size * settings.grid_size * settings.grid_size;
    if (index < count) { atomicStore(&density[index], 0u); }
}

@compute @workgroup_size(256)
fn splat_particles(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = item_index(id);
    if (index >= settings.particle_count) { return; }
    var p = particles[index].position_age.xyz;
    // Deterministic, slow volume motion without wall-clock input.
    let angle = settings.time * 0.08 * settings.motion_scale + p.y * 0.35;
    let rotated = vec2<f32>(p.x * cos(angle) - p.z * sin(angle), p.x * sin(angle) + p.z * cos(angle));
    p = vec3<f32>(rotated.x, p.y, rotated.y);
    let jitter = vec3<f32>(hash(index), hash(index + 17u), hash(index + 41u)) - vec3<f32>(0.5);
    let uvw = clamp(p * 0.32 + vec3<f32>(0.5) + jitter / f32(settings.grid_size), vec3<f32>(0.0), vec3<f32>(0.9999));
    let cell = vec3<u32>(uvw * f32(settings.grid_size));
    let voxel_index = cell.x + settings.grid_size * (cell.y + settings.grid_size * cell.z);
    atomicAdd(&density[voxel_index], 1u);
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn fullscreen(@builtin(vertex_index) index: u32) -> VertexOutput {
    let positions = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    let position = positions[index];
    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = position * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    return output;
}

fn sample_density(p: vec3<f32>) -> f32 {
    let cell = min(vec3<u32>(clamp(p, vec3<f32>(0.0), vec3<f32>(0.9999)) * f32(settings.grid_size)), vec3<u32>(settings.grid_size - 1u));
    let index = cell.x + settings.grid_size * (cell.y + settings.grid_size * cell.z);
    return min(f32(atomicLoad(&density[index])) * 0.02, 0.08);
}

@fragment
fn raymarch(input: VertexOutput) -> @location(0) vec4<f32> {
    let scaled_pixel = floor(input.position.xy / f32(settings.pixel_scale)) * f32(settings.pixel_scale);
    let uv = (scaled_pixel + vec2<f32>(0.5)) / settings.dimensions;
    var transmittance = 1.0;
    var radiance = vec3<f32>(0.0);
    let step_length = 1.0 / f32(settings.ray_steps);
    for (var step = 0u; step < settings.ray_steps; step++) {
        let z = (f32(step) + 0.5) * step_length;
        let p = vec3<f32>((uv - vec2<f32>(0.5)) * vec2<f32>(1.5, 2.4), (z - 0.5) * 2.0);
        let envelope = max(0.0, 1.0 - dot(p, p));
        let wisps = 0.55 + 0.45 * sin(p.x * 17.0 + p.z * 9.0 + settings.time * 0.3 * settings.motion_scale) * sin(p.y * 13.0 - p.z * 7.0);
        let particle_density = sample_density(vec3<f32>(uv, z));
        let d = (particle_density * envelope + envelope * envelope * max(wisps, 0.0) * 0.45) * settings.density_scale;
        let absorbed = 1.0 - exp(-d * settings.absorption * step_length);
        let color = mix(vec3<f32>(0.08, 0.02, 0.28), vec3<f32>(0.15, 0.65, 1.4), z);
        radiance += transmittance * absorbed * color * settings.emission;
        transmittance *= 1.0 - absorbed;
        if (transmittance < 0.01) { break; }
    }
    return vec4<f32>(radiance, 1.0 - transmittance);
}
