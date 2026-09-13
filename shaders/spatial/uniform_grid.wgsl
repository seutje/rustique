struct ParticlePosition {
    value: vec4<f32>,
};

struct GridParams {
    particle_count: u32,
    cells_per_axis: u32,
    cell_capacity: u32,
    max_neighbors: u32,
    world_min: vec3<f32>,
    cell_size: f32,
};

@group(0) @binding(0) var<storage, read> positions: array<ParticlePosition>;
@group(0) @binding(1) var<storage, read_write> cell_counts: array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> cell_entries: array<u32>;
@group(0) @binding(3) var<storage, read_write> neighbor_counts: array<u32>;
@group(0) @binding(4) var<storage, read_write> overflow_count: atomic<u32>;
@group(0) @binding(5) var<uniform> params: GridParams;

fn cell_coord(position: vec3<f32>) -> vec3<u32> {
    let upper = vec3<f32>(f32(params.cells_per_axis - 1u));
    return vec3<u32>(clamp(floor((position - params.world_min) / params.cell_size), vec3<f32>(0.0), upper));
}

fn cell_index(cell: vec3<u32>) -> u32 {
    return cell.x + params.cells_per_axis * (cell.y + params.cells_per_axis * cell.z);
}

fn item_index(id: vec3<u32>) -> u32 {
    return id.x + id.y * 65535u * 256u;
}

@compute @workgroup_size(256)
fn clear(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = item_index(id);
    let cell_count = params.cells_per_axis * params.cells_per_axis * params.cells_per_axis;
    if (index < cell_count) { atomicStore(&cell_counts[index], 0u); }
    if (index < params.particle_count) { neighbor_counts[index] = 0u; }
    if (index == 0u) { atomicStore(&overflow_count, 0u); }
}

@compute @workgroup_size(256)
fn build(@builtin(global_invocation_id) id: vec3<u32>) {
    let particle = item_index(id);
    if (particle >= params.particle_count) { return; }
    let cell = cell_index(cell_coord(positions[particle].value.xyz));
    let slot = atomicAdd(&cell_counts[cell], 1u);
    if (slot < params.cell_capacity) {
        cell_entries[cell * params.cell_capacity + slot] = particle;
    } else {
        atomicAdd(&overflow_count, 1u);
    }
}

@compute @workgroup_size(256)
fn query(@builtin(global_invocation_id) id: vec3<u32>) {
    let particle = item_index(id);
    if (particle >= params.particle_count) { return; }
    let position = positions[particle].value.xyz;
    let center = vec3<i32>(cell_coord(position));
    var found = 0u;
    for (var z = -1; z <= 1; z++) {
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let candidate = center + vec3<i32>(x, y, z);
                if (any(candidate < vec3<i32>(0)) || any(candidate >= vec3<i32>(i32(params.cells_per_axis)))) { continue; }
                let cell = cell_index(vec3<u32>(candidate));
                let stored = min(atomicLoad(&cell_counts[cell]), params.cell_capacity);
                for (var slot = 0u; slot < stored; slot++) {
                    let other = cell_entries[cell * params.cell_capacity + slot];
                    if (other != particle && distance(position, positions[other].value.xyz) <= params.cell_size) {
                        found++;
                        if (found >= params.max_neighbors) { break; }
                    }
                }
                if (found >= params.max_neighbors) { break; }
            }
            if (found >= params.max_neighbors) { break; }
        }
        if (found >= params.max_neighbors) { break; }
    }
    neighbor_counts[particle] = found;
}
