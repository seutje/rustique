struct Params {
    resolution_time: vec4<f32>,
    appearance: vec4<f32>, // density, size, size variation, refraction
    motion: vec4<f32>,     // Fresnel, gravity, emission, seed
    background: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;

struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    let p = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.));
    var o: Out;
    o.position = vec4(p[i], 0., 1.);
    o.uv = p[i] * vec2(.5, -.5) + .5;
    return o;
}

fn hash21(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2(123.34, 345.45));
    return fract(q.x * q.y * (q.x + q.y + 34.345));
}

fn background(uv: vec2<f32>) -> vec3<f32> {
    let base = params.background.rgb;
    let glow = vec3(.08, .32, .48) * (.25 + .75 * smoothstep(1.1, .0, distance(uv, vec2(.72, .26))));
    let bars = .08 * sin(uv.x * 18. + uv.y * 7.);
    return base + glow + vec3(bars * .25, bars * .55, bars);
}

@fragment fn fs_main(i: Out) -> @location(0) vec4<f32> {
    let aspect = params.resolution_time.x / params.resolution_time.y;
    let cells = mix(5., 14., clamp(params.appearance.x, 0., 1.));
    let p = i.uv * vec2(aspect, 1.) * cells;
    let cell = floor(p);
    var best = 10.;
    var local_best = vec2(0.);
    var radius_best = .3;

    // Each cell is one deterministic droplet instance. Emission gates the hash,
    // while gravity moves droplets down and wraps them without wall-clock state.
    for (var oy = -1; oy <= 1; oy++) {
        for (var ox = -1; ox <= 1; ox++) {
            let id = cell + vec2(f32(ox), f32(oy));
            let seeded_id = id + params.motion.w * .000001;
            let chance = hash21(seeded_id + 19.7);
            if chance > params.motion.z { continue; }
            let h = hash21(seeded_id);
            let radius = params.appearance.y * mix(1. - params.appearance.z, 1. + params.appearance.z, h);
            let fall = params.resolution_time.z * params.motion.y * mix(.45, 1.35, hash21(seeded_id + 7.3));
            var center = id + vec2(hash21(seeded_id + 2.1), fract(hash21(seeded_id + 5.9) - fall));
            let local = p - center;
            let elongation = 1. + params.motion.y * 1.8;
            let distance_to_drop = length(local * vec2(1., 1. / elongation)) / max(radius, .02);
            if distance_to_drop < best {
                best = distance_to_drop;
                local_best = local / max(radius, .02);
                radius_best = radius;
            }
        }
    }

    if best >= 1. { return vec4(background(i.uv) * params.background.w, 1.); }
    let sphere_z = sqrt(max(0., 1. - dot(local_best, local_best)));
    let normal = normalize(vec3(local_best, sphere_z));
    let distortion = normal.xy * params.appearance.w * (0.6 + radius_best);
    let refracted = background(clamp(i.uv + distortion, vec2(0.), vec2(1.)));
    let fresnel = pow(1. - max(normal.z, 0.), 5.) * params.motion.x;
    let rim = smoothstep(.72, 1., best);
    let highlight = pow(max(dot(normal, normalize(vec3(-.4, -.7, 1.))), 0.), 48.);
    let color = refracted * mix(.88, 1.12, sphere_z) + vec3(.5, .82, 1.) * fresnel + vec3(1.) * highlight * 1.8 + rim * vec3(.08, .18, .24);
    return vec4(color * params.background.w, 1.);
}
