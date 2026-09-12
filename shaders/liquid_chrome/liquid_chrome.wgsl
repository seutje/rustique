struct Params { resolution: vec2<f32>, time: f32, roughness: f32, reflection: f32, metallic: f32, surface_scale: f32, pad: f32 }
@group(0) @binding(0) var environment: texture_2d<f32>;
@group(0) @binding(1) var environment_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;
struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) i:u32)->Out { let p=array<vec2<f32>,3>(vec2(-1,-1),vec2(3,-1),vec2(-1,3)); var o:Out; o.position=vec4(p[i],0,1); o.uv=p[i]*vec2(.5,-.5)+.5; return o; }
fn sd_sphere(p:vec3<f32>,c:vec3<f32>,r:f32)->f32{return length(p-c)-r;}
fn smooth_min(a:f32,b:f32,k:f32)->f32{let h=clamp(.5+.5*(b-a)/k,0.,1.);return mix(b,a,h)-k*h*(1.-h);}
fn scene(p:vec3<f32>)->f32 { let t=params.time; let s=params.surface_scale; var d=sd_sphere(p,vec3(0.,.02*sin(t*.7),0.),.62*s); d=smooth_min(d,sd_sphere(p,vec3(.48*sin(t*.53),.34*cos(t*.71),.12*sin(t)),.38*s),.28); d=smooth_min(d,sd_sphere(p,vec3(-.43*cos(t*.47),-.31*sin(t*.83),.18*cos(t)),.34*s),.25); return d; }
fn normal(p:vec3<f32>)->vec3<f32>{let e=.002;return normalize(vec3(scene(p+vec3(e,0,0))-scene(p-vec3(e,0,0)),scene(p+vec3(0,e,0))-scene(p-vec3(0,e,0)),scene(p+vec3(0,0,e))-scene(p-vec3(0,0,e))));}
fn env(d:vec3<f32>)->vec3<f32>{let uv=vec2(atan2(d.z,d.x)/6.2831853+.5,acos(clamp(d.y,-1.,1.))/3.1415927);return textureSample(environment,environment_sampler,uv).rgb;}
@fragment fn fs_main(i:Out)->@location(0) vec4<f32>{let q=(i.uv*2.-1.)*vec2(params.resolution.x/params.resolution.y,1.);let ro=vec3(0.,0.,2.65);let rd=normalize(vec3(q,-1.75));var depth=0.;var hit=false;for(var step=0;step<96;step++){let p=ro+rd*depth;let d=scene(p);if(d<.0015){hit=true;break;}depth+=d*.75;if(depth>6.){break;}}if(!hit){return vec4(env(rd)*.32,1.);}let p=ro+rd*depth;let n=normal(p);let reflected=reflect(rd,n);let blurred=normalize(mix(reflected,n,params.roughness*.55));var color=env(blurred)*params.reflection;let fresnel=pow(1.-max(dot(-rd,n),0.),5.);let base=vec3(.22,.24,.27);color=mix(base,color,params.metallic)*(.55+.45*fresnel)+vec3(.7)*pow(max(dot(n,normalize(vec3(-.4,.8,.5))),0.),32.*(1.-params.roughness)+2.);return vec4(color,1.);}
