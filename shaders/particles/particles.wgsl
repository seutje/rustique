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
    simulation_seed: u32,
    force_count: u32,
    force_scale: f32,
    brightness: f32,
    active_particle_count: u32,
    confine_to_box: u32,
    initialization_mode: u32,
    _padding: u32,
    initialization_params: vec4<f32>,
    lifecycle_params: vec4<f32>,
    // Camera-space near/far distances followed by size/brightness strengths.
    particle_depth_response: vec4<f32>,
}

struct Force {
    kind: vec4<u32>,
    primary: vec4<f32>,
    secondary: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> particles_in: array<Particle>;
@group(0) @binding(1) var<storage, read_write> particles_out: array<Particle>;
@group(0) @binding(2) var<uniform> frame: FrameUniforms;
@group(0) @binding(3) var<storage, read> forces: array<Force>;

fn random_unit(value: u32) -> f32 {
    var mixed = value;
    mixed = (mixed ^ (mixed >> 16u)) * 0x7feb352du;
    mixed = (mixed ^ (mixed >> 15u)) * 0x846ca68bu;
    mixed = mixed ^ (mixed >> 16u);
    return f32(mixed >> 8u) / 16777216.0;
}

@compute @workgroup_size(256)
fn update(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x + id.y * 65535u * 256u;
    if (index >= frame.particle_count) { return; }
    if (index >= frame.active_particle_count) {
        particles_out[index] = particles_in[index];
        return;
    }
    var particle = particles_in[index];
    // A future spawn time keeps particles dormant during the initial stagger
    // and briefly after each respawn. Dormant particles retain their initial state.
    if (frame.simulation_time < particle.params.x) {
        particles_out[index] = particle;
        return;
    }
    var velocity = particle.velocity_lifetime.xyz;
    for (var force_index = 0u; force_index < frame.force_count; force_index++) {
        let force = forces[force_index];
        if (force.kind.x == 0u) {
            velocity += force.primary.xyz * frame.force_scale * frame.delta_time;
        } else if (force.kind.x == 1u || force.kind.x == 2u) {
            let offset = force.primary.xyz - particle.position_age.xyz;
            // Soften the singularity so close passes bend into an orbit instead of
            // launching particles into the simulation boundary.
            let distance_squared = max(dot(offset, offset), 0.01);
            let direction = offset * inverseSqrt(distance_squared);
            let polarity = select(-1.0, 1.0, force.kind.x == 1u);
            let distance = sqrt(distance_squared);
            let long_range = force.secondary.y * max(distance - 1.0, 0.0);
            let acceleration = max(force.primary.w / distance_squared, force.secondary.x) + long_range;
            velocity += direction * acceleration * frame.force_scale * polarity * frame.delta_time;
            if (force.kind.x == 1u && distance > 1.0) {
                let outward_velocity = max(dot(velocity, -direction), 0.0);
                let damping = clamp(force.secondary.z * (distance - 1.0) * frame.delta_time, 0.0, 1.0);
                velocity += direction * outward_velocity * damping;
            }
        } else if (force.kind.x == 3u) {
            let offset = particle.position_age.xyz - force.primary.xyz;
            let tangent = normalize(vec3<f32>(-offset.y, offset.x, 0.0) + vec3<f32>(0.00001, 0.0, 0.0));
            velocity += tangent * force.primary.w * frame.force_scale * frame.delta_time;
        } else if (force.kind.x == 4u) {
            velocity *= max(0.0, 1.0 - force.primary.x * frame.delta_time);
        } else if (force.kind.x == 6u) {
            let phase = particle.position_age * force.primary.y + vec4<f32>(frame.simulation_time);
            let direction = normalize(vec3<f32>(sin(phase.x + phase.y), cos(phase.y + phase.z), sin(phase.z + phase.x)));
            velocity += direction * force.primary.x * frame.force_scale * frame.delta_time;
        } else if (force.kind.x == 7u) {
            let p = particle.position_age.xyz * force.primary.y + vec3<f32>(frame.simulation_time);
            let curl = vec3<f32>(cos(p.y) - cos(p.z), cos(p.z) - cos(p.x), cos(p.x) - cos(p.y));
            velocity += curl * force.primary.x * frame.force_scale * frame.delta_time;
        } else if (force.kind.x == 8u) {
            // secondary: orbit radius, radians/second, phase radians, unused.
            let angle = force.secondary.y * frame.simulation_time + force.secondary.z;
            let well_position = force.primary.xyz
                + vec3<f32>(cos(angle), sin(angle), 0.0) * force.secondary.x;
            let offset = well_position - particle.position_age.xyz;
            let distance_squared = max(dot(offset, offset), 0.01);
            let direction = offset * inverseSqrt(distance_squared);
            velocity += direction * force.primary.w * frame.force_scale / distance_squared * frame.delta_time;
        }
    }
    particle.velocity_lifetime = vec4<f32>(velocity, particle.velocity_lifetime.w);
    particle.position_age = vec4<f32>(
        particle.position_age.xyz + particle.velocity_lifetime.xyz * frame.delta_time,
        particle.position_age.w + frame.delta_time,
    );
    for (var force_index = 0u; force_index < frame.force_count; force_index++) {
        let force = forces[force_index];
        if (force.kind.x == 5u) {
            let offset = particle.position_age.xyz - force.primary.xyz;
            let distance = length(offset);
            if (distance > force.primary.w) {
                let normal = offset / distance;
                particle.position_age = vec4<f32>(force.primary.xyz + normal * force.primary.w, particle.position_age.w);
                particle.velocity_lifetime = vec4<f32>(reflect(particle.velocity_lifetime.xyz, normal) * force.secondary.x, particle.velocity_lifetime.w);
            }
        }
    }
    if (frame.confine_to_box != 0u) {
        if (particle.position_age.x < -1.0 || particle.position_age.x > 1.0) { particle.velocity_lifetime.x *= -1.0; }
        if (particle.position_age.y < -1.0 || particle.position_age.y > 1.0) { particle.velocity_lifetime.y *= -1.0; }
        if (particle.position_age.z < -1.0 || particle.position_age.z > 1.0) { particle.velocity_lifetime.z *= -1.0; }
    }
    if (particle.position_age.w >= particle.velocity_lifetime.w) {
        let generation = frame.frame_index + 1u;
        let base = index ^ frame.simulation_seed ^ generation * 0x9e3779b9u;
        if (frame.initialization_mode == 1u) {
            let radial = sqrt(random_unit(base)) * frame.initialization_params.x;
            let angle = random_unit(base ^ 0x68bc21ebu) * 6.28318530718;
            let radial_direction = vec2<f32>(cos(angle), sin(angle));
            let z = (random_unit(base ^ 0x967a889bu) * 2.0 - 1.0)
                * frame.initialization_params.y;
            let orbital_speed = min(sqrt(0.1 / max(radial, 0.12)), 0.75);
            let speed_variation = 0.9 + random_unit(base ^ 0x02e5be93u) * 0.2;
            let speed = orbital_speed * speed_variation;
            let z_velocity = (random_unit(base ^ 0xd3a2646cu) * 2.0 - 1.0) * 0.005;
            let lifetime_scale = 1.0
                + (random_unit(base ^ 0xa511e9b3u) * 2.0 - 1.0) * frame.lifecycle_params.y;
            let respawn_delay = random_unit(base ^ 0x63d83595u) * frame.lifecycle_params.x;
            particle.position_age = vec4<f32>(radial_direction * radial, z, 0.0);
            particle.velocity_lifetime = vec4<f32>(
                -radial_direction.y * speed,
                radial_direction.x * speed,
                z_velocity,
                frame.initialization_params.z * lifetime_scale,
            );
            particle.params.x = frame.simulation_time + respawn_delay;
        } else {
            let x = random_unit(base) * 2.0 - 1.0;
            let y = random_unit(base ^ 0x68bc21ebu) * 2.0 - 1.0;
            let z = random_unit(base ^ 0x967a889bu) * 2.0 - 1.0;
            let speed = 0.05 + random_unit(base ^ 0x02e5be93u) * 0.15;
            let z_velocity = (random_unit(base ^ 0xd3a2646cu) * 2.0 - 1.0) * speed;
            particle.position_age = vec4<f32>(vec3<f32>(x, y, z) * 0.85, 0.0);
            particle.velocity_lifetime = vec4<f32>(-y * speed, x * speed, z_velocity, 5.0);
            particle.params.x = frame.simulation_time;
        }
    }
    particles_out[index] = particle;
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

fn hsv_to_rgb(hsv: vec3<f32>) -> vec3<f32> {
    let p = abs(fract(hsv.xxx + vec3<f32>(0.0, 0.6666667, 0.3333333)) * 6.0 - 3.0);
    return hsv.z * mix(vec3<f32>(1.0), clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)), hsv.y);
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var output: VertexOutput;
    let particle = particles_in[index / 6u];
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    var position = frame.view_projection * vec4<f32>(particle.position_age.xyz * frame.position_scale, 1.0);
    let view_depth = max(position.w, 0.001);
    let depth_near = frame.particle_depth_response.x;
    let depth_far = max(frame.particle_depth_response.y, depth_near + 0.001);
    let depth_mix = smoothstep(depth_near, depth_far, view_depth);
    let perspective_scale = clamp(depth_near / view_depth, 0.25, 2.0);
    let size_scale = mix(1.0, perspective_scale, frame.particle_depth_response.z);
    let offset = corners[index % 6u]
        * frame.particle_size_pixels
        * size_scale
        / frame.viewport_size;
    position = vec4<f32>(position.xy + offset * position.w, position.zw);
    if (
        index / 6u >= frame.active_particle_count
        || frame.simulation_time < particle.params.x
    ) {
        position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    output.position = position;
    var color = particle.color.rgb;
    if (frame.initialization_mode == 1u) {
        let radial = clamp(
            length(particle.position_age.xy) / max(frame.initialization_params.x, 0.001),
            0.0,
            1.25,
        );
        let gradient = smoothstep(0.0, 1.0, radial);
        let particle_variation = (particle.color.b - 0.875) * 0.12;
        let hue = mix(0.08, 0.68, gradient)
            + particle_variation
            + frame.lifecycle_params.z;
        let saturation = mix(0.35, 0.92, min(radial, 1.0));
        let value = mix(1.0, 0.58, min(radial, 1.0));
        color = hsv_to_rgb(vec3<f32>(hue, saturation, value));
    }
    let depth_brightness = 1.0 - depth_mix * frame.particle_depth_response.w;
    output.color = vec4<f32>(color * frame.brightness * depth_brightness, particle.color.a);
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
