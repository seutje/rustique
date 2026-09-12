struct Particle { position: vec4<f32>, velocity: vec4<f32> };
struct RenderParams { rest_density: f32, particle_size: f32, view_aspect: f32, padding: f32 };
@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<uniform> params: RenderParams;
struct Out { @builtin(position) position: vec4<f32>, @location(0) color: vec4<f32> };
@vertex fn vs_main(@builtin(vertex_index) vi:u32)->Out {
 let particle=vi/6u; let corner=vi%6u; let corners=array<vec2<f32>,6>(vec2(-1,-1),vec2(1,-1),vec2(-1,1),vec2(-1,1),vec2(1,-1),vec2(1,1)); let p=particles[particle]; var o:Out;
 o.position=vec4(p.position.x+corners[corner].x*params.particle_size/params.view_aspect,p.position.y+corners[corner].y*params.particle_size,p.position.z,1);
 let glow=clamp(p.position.w/params.rest_density,0.2,2.0); o.color=vec4(0.03,0.45+0.25*glow,0.08,0.7); return o;
}
@fragment fn fs_main(i:Out)->@location(0) vec4<f32>{return i.color;}
