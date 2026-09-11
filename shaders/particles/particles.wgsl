// Must remain byte-for-byte layout compatible with simulation::Particle.
struct Particle {
    position_age: vec4<f32>,
    velocity_lifetime: vec4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
}

struct FrameUniforms {
    view_projection: mat4x4<f32>,
    frame_index: u32,
    particle_count: u32,
    delta_time: f32,
    simulation_time: f32,
    viewport_size: vec2<f32>,
    particle_size_pixels: f32,
    position_scale: f32,
}

@group(0) @binding(0) var<storage, read> particles_in: array<Particle>;
@group(0) @binding(1) var<storage, read_write> particles_out: array<Particle>;
@group(0) @binding(2) var<uniform> frame: FrameUniforms;

@compute @workgroup_size(256)
fn update(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x + id.y * 65535u * 256u;
    if (index >= frame.particle_count) { return; }
    var particle = particles_in[index];
    particle.position_age = vec4<f32>(
        particle.position_age.xyz + particle.velocity_lifetime.xyz * frame.delta_time,
        particle.position_age.w + frame.delta_time,
    );
    if (particle.position_age.x < -1.0 || particle.position_age.x > 1.0) { particle.velocity_lifetime.x *= -1.0; }
    if (particle.position_age.y < -1.0 || particle.position_age.y > 1.0) { particle.velocity_lifetime.y *= -1.0; }
    if (particle.position_age.w >= particle.velocity_lifetime.w) { particle.position_age.w = 0.0; }
    particles_out[index] = particle;
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var output: VertexOutput;
    let particle = particles_in[index / 6u];
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let offset = corners[index % 6u] * frame.particle_size_pixels / frame.viewport_size;
    var position = frame.view_projection * vec4<f32>(particle.position_age.xyz * frame.position_scale, 1.0);
    position = vec4<f32>(position.xy + offset * position.w, position.zw);
    output.position = position;
    output.color = particle.color;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
