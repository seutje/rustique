struct Particle { position: vec4<f32>, velocity: vec4<f32> };
struct Params {
    particle_count: u32, cells_per_axis: u32, cell_capacity: u32, max_neighbors: u32,
    world_min: vec3<f32>, cell_size: f32,
    dt: f32, rest_density: f32, pressure: f32, viscosity: f32,
    cohesion: f32, audio_pressure: f32, audio_turbulence: f32, particle_size: f32,
    view_aspect: f32, _pad0: f32, _pad1: f32, _pad2: f32,
};
@group(0) @binding(0) var<storage, read> source: array<Particle>;
@group(0) @binding(1) var<storage, read_write> destination: array<Particle>;
@group(0) @binding(2) var<storage, read_write> cell_counts: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> cell_entries: array<u32>;
@group(0) @binding(4) var<storage, read_write> densities: array<f32>;
@group(0) @binding(5) var<uniform> params: Params;

fn coord(p: vec3<f32>) -> vec3<u32> {
    return vec3<u32>(clamp(floor((p-params.world_min)/params.cell_size), vec3<f32>(0), vec3<f32>(f32(params.cells_per_axis-1u))));
}
fn cell(c: vec3<u32>) -> u32 { return c.x + params.cells_per_axis*(c.y+params.cells_per_axis*c.z); }
fn kernel(r: f32) -> f32 { let q=max(0.0,1.0-r/params.cell_size); return q*q*q; }
fn item_index(id: vec3<u32>) -> u32 { return id.x + id.y * 65535u * 256u; }

@compute @workgroup_size(256) fn clear(@builtin(global_invocation_id) id: vec3<u32>) {
    let index=item_index(id);
    let n=params.cells_per_axis*params.cells_per_axis*params.cells_per_axis;
    if(index<n){atomicStore(&cell_counts[index],0u);} if(index<params.particle_count){densities[index]=0.0;}
}
@compute @workgroup_size(256) fn build(@builtin(global_invocation_id) id: vec3<u32>) {
    let index=item_index(id); if(index>=params.particle_count){return;} let c=cell(coord(source[index].position.xyz));
    let slot=atomicAdd(&cell_counts[c],1u); if(slot<params.cell_capacity){cell_entries[c*params.cell_capacity+slot]=index;}
}
@compute @workgroup_size(256) fn density(@builtin(global_invocation_id) id: vec3<u32>) {
    let index=item_index(id); if(index>=params.particle_count){return;} let p=source[index].position.xyz; let cc=vec3<i32>(coord(p)); var rho=1.0; var seen=0u;
    for(var z=-1;z<=1;z++){for(var y=-1;y<=1;y++){for(var x=-1;x<=1;x++){
        let q=cc+vec3<i32>(x,y,z); if(any(q<vec3<i32>(0))||any(q>=vec3<i32>(i32(params.cells_per_axis)))){continue;}
        let ci=cell(vec3<u32>(q)); let count=min(atomicLoad(&cell_counts[ci]),params.cell_capacity);
        for(var s=0u;s<count;s++){let j=cell_entries[ci*params.cell_capacity+s]; if(j!=index){rho+=kernel(distance(p,source[j].position.xyz)); seen++; if(seen>=params.max_neighbors){break;}}}
        if(seen>=params.max_neighbors){break;}
    }}}
    densities[index]=rho;
}
fn hash(v:u32)->f32 { var x=v; x^=x>>16u; x*=0x7feb352du; x^=x>>15u; return f32(x&65535u)/32767.5-1.0; }
@compute @workgroup_size(256) fn solve(@builtin(global_invocation_id) id: vec3<u32>) {
    let index=item_index(id); if(index>=params.particle_count){return;} let p=source[index].position.xyz; let v=source[index].velocity.xyz; let cc=vec3<i32>(coord(p));
    var force=vec3<f32>(0,-0.15,0); var center=vec3<f32>(0); var weight=0.0; var seen=0u;
    for(var z=-1;z<=1;z++){for(var y=-1;y<=1;y++){for(var x=-1;x<=1;x++){
        let q=cc+vec3<i32>(x,y,z); if(any(q<vec3<i32>(0))||any(q>=vec3<i32>(i32(params.cells_per_axis)))){continue;}
        let ci=cell(vec3<u32>(q)); let count=min(atomicLoad(&cell_counts[ci]),params.cell_capacity);
        for(var s=0u;s<count;s++){let j=cell_entries[ci*params.cell_capacity+s]; if(j==index){continue;} let d=p-source[j].position.xyz; let r=length(d); if(r>0.0001&&r<params.cell_size){
            let w=kernel(r); let pressure_error=max(densities[index]-params.rest_density,0.0)+max(densities[j]-params.rest_density,0.0);
            force+=normalize(d)*w*pressure_error*params.pressure*(1.0+params.audio_pressure);
            force+=(source[j].velocity.xyz-v)*w*params.viscosity; center+=source[j].position.xyz*w; weight+=w;
        } seen++; if(seen>=params.max_neighbors){break;}}
        if(seen>=params.max_neighbors){break;}
    }}}
    if(weight>0.0){force+=(center/weight-p)*params.cohesion;}
    force+=vec3<f32>(hash(index),hash(index+17u),hash(index+41u))*params.audio_turbulence;
    var nv=(v+force*params.dt)*0.998; var np=p+nv*params.dt;
    for(var axis=0u;axis<3u;axis++){if(np[axis]<-0.95){np[axis]=-0.95;nv[axis]=abs(nv[axis])*0.35;} if(np[axis]>0.95){np[axis]=0.95;nv[axis]=-abs(nv[axis])*0.35;}}
    destination[index].position=vec4<f32>(np,densities[index]); destination[index].velocity=vec4<f32>(nv,1.0);
}
