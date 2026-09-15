use bytemuck::{Pod, Zeroable};
use simulation::FlockingConfig;
use wgpu::util::DeviceExt;

use crate::{GpuContext, dispatch::dispatch_dimensions};

const SHADER: &str = include_str!("../../../shaders/particles/flocking.wgsl");
const MAX_POINTS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct FlockingModulation {
    pub separation: f32,
    pub cohesion: f32,
    pub turbulence: f32,
    pub speed: f32,
    pub randomness: f32,
    pub impulse: f32,
}

impl Default for FlockingModulation {
    fn default() -> Self {
        Self {
            separation: 1.0,
            cohesion: 1.0,
            turbulence: 1.0,
            speed: 1.0,
            randomness: 1.0,
            impulse: 0.0,
        }
    }
}

/// Atomic fixed-point hash bucket shared with `flocking.wgsl` (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FieldCell {
    values: [u32; 8],
}

const _: () = assert!(size_of::<FieldCell>() == 32);

/// Uniform layout shared with `flocking.wgsl` (336 bytes).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FlockingUniforms {
    world_min_enabled: [f32; 4],
    world_max_unused: [f32; 4],
    strengths: [f32; 4],
    noise: [f32; 4],
    motion: [f32; 4],
    attractor: [f32; 4],
    directional_boundary: [f32; 4],
    repulsor_audio: [f32; 4],
    audio: [f32; 4],
    timing: [f32; 4],
    counts: [u32; 4],
    mode: [u32; 4],
    grid: [u32; 4],
    attractors: [[f32; 4]; MAX_POINTS],
    repulsors: [[f32; 4]; MAX_POINTS],
}

const _: () = assert!(size_of::<FlockingUniforms>() == 336);

pub(crate) struct FlockingResources {
    config: FlockingConfig,
    bucket_count: u32,
    uniform_buffer: wgpu::Buffer,
    bind_groups: [wgpu::BindGroup; 2],
    clear_pipeline: wgpu::ComputePipeline,
    claim_pipeline: wgpu::ComputePipeline,
    accumulate_pipeline: wgpu::ComputePipeline,
    apply_pipeline: wgpu::ComputePipeline,
}

impl FlockingResources {
    pub(crate) fn new(
        context: &GpuContext,
        buffers: &[wgpu::Buffer; 2],
        particle_count: u32,
        config: FlockingConfig,
    ) -> Option<Self> {
        if !config.enabled || config.grid_resolution < 4 {
            return None;
        }
        let logical_cell_count = config.grid_resolution.checked_pow(3)?;
        // Keep the sparse field at or below 64 MiB while maintaining at least two
        // buckets per accumulated field leader at high logical resolutions.
        let leader_count = particle_count.min(900_000);
        let desired_buckets = leader_count.saturating_mul(2).max(1).next_power_of_two();
        let bucket_count = logical_cell_count.min(desired_buckets);
        let cells = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flocking-aggregate-field"),
            size: u64::from(bucket_count) * size_of::<FieldCell>() as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let uniform_buffer = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("flocking-uniforms"),
                contents: bytemuck::bytes_of(&FlockingUniforms::zeroed()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let layout = context
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("flocking-bind-layout"),
                entries: &[
                    storage_entry(0, true),
                    storage_entry(1, false),
                    storage_entry(2, false),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let bind_groups = [
            bind_group(
                context,
                &layout,
                &buffers[0],
                &buffers[1],
                &cells,
                &uniform_buffer,
                "flocking-a-b",
            ),
            bind_group(
                context,
                &layout,
                &buffers[1],
                &buffers[0],
                &cells,
                &uniform_buffer,
                "flocking-b-a",
            ),
        ];
        let module = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("flocking-shader"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let pipeline_layout =
            context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("flocking-pipeline-layout"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                });
        let pipeline = |entry_point| {
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry_point),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(entry_point),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                })
        };
        Some(Self {
            config,
            bucket_count,
            uniform_buffer,
            bind_groups,
            clear_pipeline: pipeline("clear_field"),
            claim_pipeline: pipeline("claim_field"),
            accumulate_pipeline: pipeline("accumulate_field"),
            apply_pipeline: pipeline("apply_flocking"),
        })
    }

    #[allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &self,
        context: &GpuContext,
        particle_count: u32,
        active_particle_count: u32,
        simulation_time: f32,
        delta_time: f32,
        seed: u32,
        modulation: FlockingModulation,
    ) {
        let mut attractors = [[0.0; 4]; MAX_POINTS];
        let mut repulsors = [[0.0; 4]; MAX_POINTS];
        for (output, input) in attractors.iter_mut().zip(&self.config.attractors) {
            output[..3].copy_from_slice(input);
        }
        for (output, input) in repulsors.iter_mut().zip(&self.config.repulsors) {
            output[..3].copy_from_slice(input);
        }
        let uniforms = FlockingUniforms {
            world_min_enabled: extend(
                self.config.world_min,
                if self.config.enabled { 1.0 } else { 0.0 },
            ),
            world_max_unused: extend(self.config.world_max, 0.0),
            strengths: [
                self.config.separation_strength,
                self.config.alignment_strength,
                self.config.cohesion_strength,
                self.config.neighborhood_radius,
            ],
            noise: [
                self.config.noise_strength,
                self.config.noise_scale,
                self.config.noise_evolution_speed,
                self.config.randomness,
            ],
            motion: [
                self.config.inertia,
                self.config.drag,
                self.config.max_velocity,
                self.config.max_steering_force,
            ],
            attractor: [
                self.config.attractor_strength,
                self.config.attractor_radius,
                self.config.swarm_compactness,
                self.config.boundary_avoidance_strength,
            ],
            directional_boundary: extend(self.config.directional_bias, self.config.boundary_margin),
            repulsor_audio: [
                self.config.repulsor_strength,
                self.config.repulsor_radius,
                modulation.separation,
                modulation.cohesion,
            ],
            audio: [
                modulation.turbulence,
                modulation.speed,
                modulation.randomness,
                modulation.impulse,
            ],
            timing: [
                simulation_time,
                delta_time,
                self.config.murmuration.state_duration_seconds,
                self.config.murmuration.transition_duration_seconds,
            ],
            counts: [
                particle_count,
                active_particle_count,
                self.config.attractors.len().min(MAX_POINTS) as u32,
                self.config.repulsors.len().min(MAX_POINTS) as u32,
            ],
            mode: [
                u32::from(self.config.murmuration.enabled),
                self.config.grid_resolution,
                seed,
                active_particle_count.div_ceil(900_000).max(1),
            ],
            grid: [self.bucket_count, 0, 0, 0],
            attractors,
            repulsors,
        };
        context
            .queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Encodes field construction and steering. The caller then runs the
    /// regular force/integration pass with the opposite ping-pong source.
    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source_index: usize,
        particle_count: u32,
    ) {
        for (pipeline, count, label) in [
            (&self.clear_pipeline, self.bucket_count, "flocking-clear"),
            (&self.claim_pipeline, particle_count, "flocking-claim"),
            (
                &self.accumulate_pipeline,
                particle_count,
                "flocking-accumulate",
            ),
            (&self.apply_pipeline, particle_count, "flocking-steer"),
        ] {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(label),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.bind_groups[source_index], &[]);
            let (groups_x, groups_y) = dispatch_dimensions(count);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
    }
}

const fn extend(value: [f32; 3], fourth: f32) -> [f32; 4] {
    [value[0], value[1], value[2], fourth]
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bind_group<'a>(
    context: &GpuContext,
    layout: &wgpu::BindGroupLayout,
    input: &'a wgpu::Buffer,
    output: &'a wgpu::Buffer,
    cells: &'a wgpu::Buffer,
    uniforms: &'a wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    context
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                entry(0, input),
                entry(1, output),
                entry(2, cells),
                entry(3, uniforms),
            ],
        })
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}
