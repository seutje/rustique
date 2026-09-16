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
    // Fire data is kept in the shared frame uniform so the existing single
    // compute/render pass remains sufficient for millions of particles.
    fire_base: vec4<f32>,
    fire_style: vec4<f32>,
    fire_audio_body: vec4<f32>,
    fire_audio_detail: vec4<f32>,
    fire_audio_accent: vec4<f32>,
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

fn fire_curl(p: vec3<f32>, time: f32) -> vec3<f32> {
    let q = p + vec3<f32>(time * 0.31, -time * 0.23, time * 0.19);
    return vec3<f32>(
        cos(q.y + sin(q.z)) - cos(q.z * 1.17),
        (cos(q.z + sin(q.x)) - cos(q.x * 0.91)) * 0.28,
        cos(q.x + sin(q.y)) - cos(q.y * 1.09),
    );
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
    if (frame.initialization_mode == 2u) {
        let age01 = clamp(particle.position_age.w / max(particle.velocity_lifetime.w, 0.001), 0.0, 1.0);
        let audio = frame.fire_base.w;
        let body = mix(vec4<f32>(1.0), frame.fire_audio_body, vec4<f32>(audio));
        let detail = mix(vec4<f32>(1.0, 1.0, 1.0, 0.0), frame.fire_audio_detail, vec4<f32>(audio));
        let is_spark = particle.params.y >= 1.0;
        if (is_spark) {
            // Embers retain coherent arcs; only a little fine-scale air motion
            // is applied and gravity wins near the end of their life.
            velocity.y -= mix(0.16, 0.42, age01) * frame.delta_time;
            velocity += fire_curl(particle.position_age.xyz * 5.5, frame.simulation_time)
                * frame.fire_base.y * (0.05 + detail.z * 0.035) * frame.delta_time;
        } else {
            let height = clamp((particle.position_age.y + 0.86) / 1.8, 0.0, 1.0);
            let coarse = fire_curl(particle.position_age.xyz * 0.35, frame.simulation_time * 0.42);
            let medium = fire_curl(particle.position_age.xyz * 1.45, frame.simulation_time * 0.9);
            let fine = fire_curl(particle.position_age.xyz * 6.0, frame.simulation_time * 2.1);
            let broad_sway = frame.fire_base.y * (0.22 + 0.20 * body.w);
            let medium_turbulence = frame.fire_base.y * (0.52 + 0.38 * detail.x) * (0.75 + height * 0.9);
            let edge_flicker = frame.fire_base.z * (0.10 + 0.10 * detail.y);
            let tip_shimmer = frame.fire_base.z * detail.z * height * height * 0.09;
            velocity += (coarse * broad_sway + medium * medium_turbulence
                + fine * (edge_flicker + tip_shimmer)) * frame.delta_time;
            velocity.y += frame.fire_base.x * body.z * (1.0 - age01 * 0.35) * frame.delta_time;
            // A tight base becomes increasingly free to split into persistent
            // tongues higher up, avoiding Brownian motion in the flame body.
            let radial = vec2<f32>(particle.position_age.x, particle.position_age.z);
            let radial_force = radial * (1.15 + 1.1 * height) * frame.delta_time;
            velocity.x -= radial_force.x;
            velocity.z -= radial_force.y;
            let audio_expansion = radial * max(body.y - 1.0, 0.0) * 0.7 * frame.delta_time;
            velocity.x += audio_expansion.x;
            velocity.z += audio_expansion.y;
            var tongue = 0.0;
            if (particle.params.z < 0.333) { tongue = -1.0; }
            if (particle.params.z > 0.666) { tongue = 1.0; }
            let tongue_target = tongue * height * height * (0.22 + detail.x * 0.09)
                + sin(height * 7.0 - frame.simulation_time * 2.3 + tongue) * height * 0.12;
            velocity.x += (tongue_target - particle.position_age.x)
                * height * (3.6 + detail.x) * frame.delta_time;
            let wave_position = fract(frame.simulation_time * 0.72);
            let wave = exp(-pow((height - wave_position) * 13.0, 2.0));
            velocity.y += wave * frame.fire_style.y * frame.fire_audio_accent.y * 0.8 * frame.delta_time;
        }
    }
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
        } else if (frame.initialization_mode == 2u) {
            let audio = frame.fire_base.w;
            let body = mix(vec4<f32>(1.0), frame.fire_audio_body, vec4<f32>(audio));
            let detail = mix(vec4<f32>(1.0, 1.0, 1.0, 0.0), frame.fire_audio_detail, vec4<f32>(audio));
            let radius = sqrt(random_unit(base)) * frame.initialization_params.x * body.y;
            let angle = random_unit(base ^ 0x68bc21ebu) * 6.28318530718;
            let direction = vec2<f32>(cos(angle), sin(angle));
            let spark_chance = clamp(frame.lifecycle_params.x + detail.w * 0.18, 0.0, 0.5);
            let spark_pick = random_unit(base ^ 0x967a889bu);
            let is_spark = spark_pick < spark_chance;
            let high_weight = max(detail.z - 0.9, 0.0) * 1.7;
            let mid_weight = max(detail.x - 0.9, 0.0) * 1.2;
            let class_pick = random_unit(base ^ 0xd3a2646cu);
            var spark_class = 1.0;
            if (class_pick < high_weight / max(high_weight + mid_weight + body.z, 0.001)) {
                spark_class = 3.0;
            } else if (class_pick < (high_weight + mid_weight) / max(high_weight + mid_weight + body.z, 0.001)) {
                spark_class = 2.0;
            }
            let life_scale = 1.0 + (random_unit(base ^ 0xa511e9b3u) * 2.0 - 1.0)
                * frame.initialization_params.w;
            var lifetime = frame.initialization_params.z * life_scale;
            var upward = frame.initialization_params.y * body.z
                * (0.72 + random_unit(base ^ 0x02e5be93u) * 0.4);
            var lateral = 0.06;
            if (is_spark) {
                lifetime = frame.lifecycle_params.z * life_scale
                    * select(1.25, select(0.85, 0.55, spark_class > 2.5), spark_class > 1.5);
                upward = frame.lifecycle_params.y * (0.7 + random_unit(base ^ 0x02e5be93u) * 0.6)
                    * select(0.82, select(1.0, 1.3, spark_class > 2.5), spark_class > 1.5);
                lateral = select(0.12, select(0.2, 0.13, spark_class > 2.5), spark_class > 1.5);
            }
            particle.position_age = vec4<f32>(direction.x * radius, -0.86, direction.y * radius * 0.55, 0.0);
            particle.velocity_lifetime = vec4<f32>(
                (random_unit(base ^ 0x63d83595u) * 2.0 - 1.0) * lateral,
                upward,
                (random_unit(base ^ 0xb5297a4du) * 2.0 - 1.0) * lateral * 0.65,
                lifetime,
            );
            let emission_delay = random_unit(base ^ 0x7f4a7c15u)
                * max(1.0 / max(body.x, 0.25) - 1.0, 0.0)
                * 0.32;
            particle.params = vec4<f32>(frame.simulation_time + emission_delay, select(0.0, spark_class, is_spark), random_unit(base ^ 0x1b56c4e9u), random_unit(base ^ 0xc2b2ae35u));
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
    @location(1) quad: vec2<f32>,
    @location(2) fire: f32,
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
    let corner = corners[index % 6u];
    var particle_size = frame.particle_size_pixels;
    if (frame.initialization_mode == 2u && particle.params.y >= 1.0) {
        particle_size *= select(0.72, select(0.52, 0.32, particle.params.y > 2.5), particle.params.y > 1.5);
    }
    let offset = corner
        * particle_size
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
    output.quad = corner;
    output.fire = select(0.0, 1.0, frame.initialization_mode == 2u);
    var color = particle.color.rgb;
    var alpha = particle.color.a;
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
    } else if (frame.initialization_mode == 2u) {
        let age01 = clamp(particle.position_age.w / max(particle.velocity_lifetime.w, 0.001), 0.0, 1.0);
        let audio_temperature = frame.fire_audio_accent.x * frame.fire_base.w;
        let temperature = clamp(frame.fire_style.x + audio_temperature * 0.28 + particle.params.w * 0.08, 0.0, 1.0);
        let hot = vec3<f32>(1.0, mix(0.72, 0.96, temperature), mix(0.08, 0.58, temperature));
        let warm = vec3<f32>(1.0, mix(0.16, 0.46, temperature), 0.015);
        let cool = vec3<f32>(0.34, 0.018, 0.002);
        color = mix(mix(hot, warm, smoothstep(0.05, 0.58, age01)), cool, smoothstep(0.58, 1.0, age01));
        if (particle.params.y >= 1.0) {
            color = mix(warm, hot, select(0.3, 0.88, particle.params.y > 2.5));
        }
        let emission = mix(1.0, frame.fire_audio_body.x, frame.fire_base.w);
        color *= 0.82 + emission * 0.18;
        alpha = select(
            mix(0.06, 0.006, smoothstep(0.22, 1.0, age01)),
            mix(0.34, 0.08, age01),
            particle.params.y >= 1.0,
        );
        if (particle.params.y < 1.0) {
            let height = clamp((particle.position_age.y + 0.86) / 1.8, 0.0, 1.0);
            let structure = sin(particle.position_age.x * 8.0 + frame.simulation_time * 2.1)
                + sin(particle.position_age.z * 10.0 - frame.simulation_time * 1.7)
                + sin(height * 13.0 - frame.simulation_time * 3.2);
            let tongue_mask = smoothstep(-1.1, 0.55, structure - height * 0.7);
            alpha *= mix(1.0, 0.18 + tongue_mask * 0.82, smoothstep(0.28, 0.9, height));
        }
    }
    let depth_brightness = 1.0 - depth_mix * frame.particle_depth_response.w;
    output.color = vec4<f32>(color * frame.brightness * depth_brightness, alpha);
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if (input.fire > 0.5) {
        let radius = length(input.quad);
        let alpha = 1.0 - smoothstep(0.18, 1.0, radius);
        if (alpha <= 0.002) { discard; }
        return vec4<f32>(input.color.rgb, input.color.a * alpha);
    }
    return input.color;
}
