// Aggregate-field flocking. Each cell stores fixed-point sums so this works on
// WebGPU implementations without floating-point atomics. POSITION_SCALE is
// deliberately conservative: even one million particles in one cell cannot
// overflow a signed 32-bit sum within the configured world/velocity limits.
const FIELD_SCALE: f32 = 512.0;

struct Particle {
    position_age: vec4<f32>,
    velocity_lifetime: vec4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
}

struct FieldCell {
    key: atomic<u32>,
    count: atomic<u32>,
    position_x: atomic<i32>,
    position_y: atomic<i32>,
    position_z: atomic<i32>,
    velocity_x: atomic<i32>,
    velocity_y: atomic<i32>,
    velocity_z: atomic<i32>,
}

struct FlockingUniforms {
    world_min_enabled: vec4<f32>,
    world_max_unused: vec4<f32>,
    strengths: vec4<f32>,
    noise: vec4<f32>,
    motion: vec4<f32>,
    attractor: vec4<f32>,
    directional_boundary: vec4<f32>,
    repulsor_audio: vec4<f32>,
    audio: vec4<f32>,
    timing: vec4<f32>,
    counts: vec4<u32>,
    mode: vec4<u32>,
    grid: vec4<u32>,
    attractors: array<vec4<f32>, 4>,
    repulsors: array<vec4<f32>, 4>,
}

struct FieldSample {
    count: f32,
    position_sum: vec3<f32>,
    velocity_sum: vec3<f32>,
}

struct Behavior {
    weights: vec4<f32>,
    modes: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> particles_in: array<Particle>;
@group(0) @binding(1) var<storage, read_write> particles_out: array<Particle>;
@group(0) @binding(2) var<storage, read_write> field: array<FieldCell>;
@group(0) @binding(3) var<uniform> params: FlockingUniforms;

fn item_index(id: vec3<u32>) -> u32 {
    return id.x + id.y * 65535u * 256u;
}

fn hash(value: u32) -> u32 {
    var mixed = value;
    mixed = (mixed ^ (mixed >> 16u)) * 0x7feb352du;
    mixed = (mixed ^ (mixed >> 15u)) * 0x846ca68bu;
    return mixed ^ (mixed >> 16u);
}

fn random_signed(value: u32) -> f32 {
    return f32(hash(value) >> 8u) / 8388608.0 - 1.0;
}

fn grid_resolution() -> u32 { return params.mode.y; }

fn cell_coord(position: vec3<f32>) -> vec3<i32> {
    let extent = params.world_max_unused.xyz - params.world_min_enabled.xyz;
    let normalized = (position - params.world_min_enabled.xyz) / extent;
    let upper = f32(grid_resolution() - 1u);
    return vec3<i32>(clamp(floor(normalized * f32(grid_resolution())), vec3<f32>(0.0), vec3<f32>(upper)));
}

fn valid_cell(cell: vec3<i32>) -> bool {
    return all(cell >= vec3<i32>(0)) && all(cell < vec3<i32>(i32(grid_resolution())));
}

fn logical_cell_index(cell: vec3<i32>) -> u32 {
    let c = vec3<u32>(cell);
    return c.x + grid_resolution() * (c.y + grid_resolution() * c.z);
}

fn cell_key(cell: vec3<i32>) -> u32 {
    // 1024^3 fits below u32::MAX, so adding one leaves zero available as the
    // empty-bucket sentinel and uniquely identifies every logical cell.
    return logical_cell_index(cell) + 1u;
}

fn bucket_index(cell: vec3<i32>) -> u32 {
    let logical_count = grid_resolution() * grid_resolution() * grid_resolution();
    let logical_index = logical_cell_index(cell);
    if (logical_count <= params.grid.x) { return logical_index; }
    return hash(logical_index) % params.grid.x;
}

fn sample_cell(cell: vec3<i32>) -> FieldSample {
    var result: FieldSample;
    result.count = 0.0;
    result.position_sum = vec3<f32>(0.0);
    result.velocity_sum = vec3<f32>(0.0);
    if (!valid_cell(cell)) { return result; }
    let index = bucket_index(cell);
    if (atomicLoad(&field[index].key) != cell_key(cell)) { return result; }
    result.count = f32(atomicLoad(&field[index].count));
    result.position_sum = vec3<f32>(
        f32(atomicLoad(&field[index].position_x)),
        f32(atomicLoad(&field[index].position_y)),
        f32(atomicLoad(&field[index].position_z)),
    ) / FIELD_SCALE;
    result.velocity_sum = vec3<f32>(
        f32(atomicLoad(&field[index].velocity_x)),
        f32(atomicLoad(&field[index].velocity_y)),
        f32(atomicLoad(&field[index].velocity_z)),
    ) / FIELD_SCALE;
    return result;
}

fn add_samples(a: FieldSample, b: FieldSample) -> FieldSample {
    var result: FieldSample;
    result.count = a.count + b.count;
    result.position_sum = a.position_sum + b.position_sum;
    result.velocity_sum = a.velocity_sum + b.velocity_sum;
    return result;
}

fn behavior_for(state: u32) -> Behavior {
    var result: Behavior;
    // weights: separation, alignment, cohesion, curl/noise
    // modes: directional stream, expansion, split, attractor
    switch state {
        case 0u: { result.weights = vec4<f32>(0.9, 1.15, 1.35, 0.65); result.modes = vec4<f32>(0.2, 0.0, 0.0, 1.1); }
        case 1u: { result.weights = vec4<f32>(0.8, 1.35, 0.75, 0.8); result.modes = vec4<f32>(1.8, 0.0, 0.0, 0.7); }
        case 2u: { result.weights = vec4<f32>(1.0, 0.75, 0.65, 2.1); result.modes = vec4<f32>(0.35, 0.0, 0.0, 0.65); }
        case 3u: { result.weights = vec4<f32>(2.0, 0.55, 0.25, 1.25); result.modes = vec4<f32>(0.15, 1.7, 0.0, 0.25); }
        case 4u: { result.weights = vec4<f32>(1.35, 0.9, 0.55, 1.0); result.modes = vec4<f32>(0.5, 0.25, 1.0, 1.55); }
        default: { result.weights = vec4<f32>(0.75, 1.2, 1.75, 0.55); result.modes = vec4<f32>(0.15, 0.0, 0.0, 1.8); }
    }
    return result;
}

fn current_behavior() -> Behavior {
    if (params.mode.x == 0u) {
        var base: Behavior;
        base.weights = vec4<f32>(1.0);
        base.modes = vec4<f32>(1.0, 0.0, 0.0, 1.0);
        return base;
    }
    let duration = max(params.timing.z, 0.001);
    let local_time = params.timing.x - floor(params.timing.x / duration) * duration;
    let state = u32(floor(params.timing.x / duration)) % 6u;
    let transition_start = max(duration - params.timing.w, 0.0);
    let blend = smoothstep(transition_start, duration, local_time);
    let a = behavior_for(state);
    let b = behavior_for((state + 1u) % 6u);
    var result: Behavior;
    result.weights = mix(a.weights, b.weights, blend);
    result.modes = mix(a.modes, b.modes, blend);
    return result;
}

fn curl_field(position: vec3<f32>, time: f32) -> vec3<f32> {
    let p = position * params.noise.y + vec3<f32>(time * params.noise.z, time * params.noise.z * 0.73, -time * params.noise.z * 0.91);
    let first = vec3<f32>(cos(p.y) - cos(p.z), cos(p.z) - cos(p.x), cos(p.x) - cos(p.y));
    let q = p * 1.93 + vec3<f32>(2.1, -1.4, 0.7);
    let second = vec3<f32>(cos(q.y) - cos(q.z), cos(q.z) - cos(q.x), cos(q.x) - cos(q.y));
    return first + second * 0.42;
}

fn moving_attractor(index: u32) -> vec3<f32> {
    let base = select(vec3<f32>(0.0), params.attractors[index].xyz, params.counts.z > 0u);
    let phase = params.timing.x * (0.11 + f32(index) * 0.027) + f32(index) * 2.39996;
    let orbit = params.attractor.y * vec3<f32>(cos(phase), sin(phase * 0.83), sin(phase * 0.61)) * 0.35;
    return base + orbit;
}

fn clamp_length(value: vec3<f32>, maximum: f32) -> vec3<f32> {
    let magnitude_squared = dot(value, value);
    if (magnitude_squared > maximum * maximum) {
        return value * maximum * inverseSqrt(magnitude_squared);
    }
    return value;
}

@compute @workgroup_size(256)
fn clear_field(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = item_index(id);
    if (index >= params.grid.x) { return; }
    let logical_count = grid_resolution() * grid_resolution() * grid_resolution();
    atomicStore(&field[index].key, select(0u, index + 1u, logical_count <= params.grid.x));
    atomicStore(&field[index].count, 0u);
    atomicStore(&field[index].position_x, 0);
    atomicStore(&field[index].position_y, 0);
    atomicStore(&field[index].position_z, 0);
    atomicStore(&field[index].velocity_x, 0);
    atomicStore(&field[index].velocity_y, 0);
    atomicStore(&field[index].velocity_z, 0);
}

@compute @workgroup_size(256)
fn claim_field(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = item_index(id);
    if (index >= params.counts.x || index >= params.counts.y) { return; }
    let logical_count = grid_resolution() * grid_resolution() * grid_resolution();
    if (logical_count <= params.grid.x) { return; }
    // Above roughly one million particles, a deterministic subset acts as
    // field leaders. All particles still sample the field, avoiding atomic
    // overflow and preserving stable field memory at extreme counts.
    if (hash(index ^ params.mode.z) % params.mode.w != 0u) { return; }
    let particle = particles_in[index];
    if (params.timing.x < particle.params.x) { return; }
    let coord = cell_coord(particle.position_age.xyz);
    let cell = bucket_index(coord);
    let key = cell_key(coord);
    let _previous = atomicMax(&field[cell].key, key);
}

@compute @workgroup_size(256)
fn accumulate_field(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = item_index(id);
    if (index >= params.counts.x || index >= params.counts.y) { return; }
    if (hash(index ^ params.mode.z) % params.mode.w != 0u) { return; }
    let particle = particles_in[index];
    if (params.timing.x < particle.params.x) { return; }
    let coord = cell_coord(particle.position_age.xyz);
    let cell = bucket_index(coord);
    let key = cell_key(coord);
    // A different logical cell won this hash bucket during the claim pass.
    // Dropping this splat avoids mixing unrelated distant neighborhoods.
    if (atomicLoad(&field[cell].key) != key) { return; }
    let position = clamp(particle.position_age.xyz, vec3<f32>(-4.0), vec3<f32>(4.0));
    let velocity = clamp(particle.velocity_lifetime.xyz, vec3<f32>(-4.0), vec3<f32>(4.0));
    atomicAdd(&field[cell].count, 1u);
    atomicAdd(&field[cell].position_x, i32(round(position.x * FIELD_SCALE)));
    atomicAdd(&field[cell].position_y, i32(round(position.y * FIELD_SCALE)));
    atomicAdd(&field[cell].position_z, i32(round(position.z * FIELD_SCALE)));
    atomicAdd(&field[cell].velocity_x, i32(round(velocity.x * FIELD_SCALE)));
    atomicAdd(&field[cell].velocity_y, i32(round(velocity.y * FIELD_SCALE)));
    atomicAdd(&field[cell].velocity_z, i32(round(velocity.z * FIELD_SCALE)));
}

@compute @workgroup_size(256)
fn apply_flocking(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = item_index(id);
    if (index >= params.counts.x) { return; }
    var particle = particles_in[index];
    if (index >= params.counts.y || params.timing.x < particle.params.x || params.world_min_enabled.w == 0.0) {
        particles_out[index] = particle;
        return;
    }
    let position = particle.position_age.xyz;
    var velocity = particle.velocity_lifetime.xyz;
    let center = cell_coord(position);
    let cell_width = min(min(
        (params.world_max_unused.x - params.world_min_enabled.x) / f32(grid_resolution()),
        (params.world_max_unused.y - params.world_min_enabled.y) / f32(grid_resolution())),
        (params.world_max_unused.z - params.world_min_enabled.z) / f32(grid_resolution()));
    let offset = i32(clamp(ceil(params.strengths.w / max(cell_width, 0.0001)), 1.0, 2.0));
    let xp = sample_cell(center + vec3<i32>(offset, 0, 0));
    let xn = sample_cell(center - vec3<i32>(offset, 0, 0));
    let yp = sample_cell(center + vec3<i32>(0, offset, 0));
    let yn = sample_cell(center - vec3<i32>(0, offset, 0));
    let zp = sample_cell(center + vec3<i32>(0, 0, offset));
    let zn = sample_cell(center - vec3<i32>(0, 0, offset));
    var local = sample_cell(center);
    local = add_samples(local, xp); local = add_samples(local, xn);
    local = add_samples(local, yp); local = add_samples(local, yn);
    local = add_samples(local, zp); local = add_samples(local, zn);

    let behavior = current_behavior();
    var steering = vec3<f32>(0.0);
    if (local.count > 1.0) {
        let average_position = local.position_sum / local.count;
        let average_velocity = local.velocity_sum / local.count;
        let density_gradient = vec3<f32>(xp.count - xn.count, yp.count - yn.count, zp.count - zn.count);
        let away_from_center = position - average_position;
        let away = select(-normalize(density_gradient + vec3<f32>(0.00001, 0.0, 0.0)), normalize(away_from_center), length(away_from_center) > 0.0001);
        let cells = f32(grid_resolution() * grid_resolution() * grid_resolution());
        let expected_neighbors = f32(params.counts.y) / f32(params.mode.w) / cells * 7.0;
        let desired_density = max(expected_neighbors * (0.5 + params.attractor.z * 1.5), 2.0);
        // Only strongly separate when a neighborhood is denser than the
        // configured flock compactness. A small floor still prevents collapse
        // without driving the flock into a uniform box-filling distribution.
        let crowding = clamp((local.count / desired_density - 0.65) / 1.5, 0.035, 1.0);
        steering += away * params.strengths.x * params.repulsor_audio.z * behavior.weights.x * crowding;
        steering += (average_velocity - velocity) * params.strengths.y * behavior.weights.y;
        steering += (average_position - position) * params.strengths.z * params.repulsor_audio.w * behavior.weights.z;
    }

    steering += curl_field(position, params.timing.x) * params.noise.x * params.audio.x * behavior.weights.w;
    steering += params.directional_boundary.xyz * behavior.modes.x;

    let attractor_count = max(params.counts.z, 1u);
    var mean_attractor = vec3<f32>(0.0);
    for (var attractor_index = 0u; attractor_index < attractor_count; attractor_index++) {
        mean_attractor += moving_attractor(attractor_index);
    }
    mean_attractor /= f32(attractor_count);
    let assigned = moving_attractor(hash(index ^ params.mode.z) % attractor_count);
    let attractor_target = mix(mean_attractor, assigned, behavior.modes.z);
    let target_offset = attractor_target - position;
    let target_distance = length(target_offset);
    if (target_distance > 0.0001) {
        let falloff = min(target_distance / max(params.attractor.y, 0.001), 1.0);
        steering += target_offset / target_distance * params.attractor.x * behavior.modes.w * falloff;
        steering -= target_offset / target_distance * params.attractor.x * behavior.modes.y;
    }

    for (var repulsor_index = 0u; repulsor_index < params.counts.w; repulsor_index++) {
        let offset_from_repulsor = position - params.repulsors[repulsor_index].xyz;
        let distance = length(offset_from_repulsor);
        if (distance < params.repulsor_audio.y && distance > 0.0001) {
            let pressure = 1.0 - distance / params.repulsor_audio.y;
            steering += offset_from_repulsor / distance * params.repulsor_audio.x * pressure * pressure;
        }
    }

    let impulse_direction = normalize(position - mean_attractor + vec3<f32>(0.00001, 0.0, 0.0));
    let impulse_wave = 0.5 + 0.5 * sin(length(position - mean_attractor) * 22.0 - params.timing.x * 9.0);
    steering += impulse_direction * params.audio.w * impulse_wave * 3.0;

    let margin = params.directional_boundary.w;
    let from_min = position - params.world_min_enabled.xyz;
    let from_max = params.world_max_unused.xyz - position;
    for (var axis = 0u; axis < 3u; axis++) {
        if (from_min[axis] < margin) { steering[axis] += params.attractor.w * (1.0 - from_min[axis] / margin); }
        if (from_max[axis] < margin) { steering[axis] -= params.attractor.w * (1.0 - from_max[axis] / margin); }
    }

    let random_epoch = u32(floor(params.timing.x * 24.0));
    let variation = vec3<f32>(
        random_signed(index ^ random_epoch ^ params.mode.z),
        random_signed(index ^ random_epoch * 3u ^ 0x68bc21ebu),
        random_signed(index ^ random_epoch * 7u ^ 0x967a889bu),
    );
    steering += variation * params.noise.w * params.audio.z;
    steering = clamp_length(steering, params.motion.w);
    velocity += steering * params.timing.y / max(params.motion.x, 0.05);
    velocity *= exp(-params.motion.y * params.timing.y);
    velocity = clamp_length(velocity, params.motion.z * max(params.audio.y, 0.05));
    particle.velocity_lifetime = vec4<f32>(velocity, particle.velocity_lifetime.w);
    particles_out[index] = particle;
}
