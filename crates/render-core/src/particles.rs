use std::{path::Path, sync::mpsc, time::Instant};

use bytemuck::{Pod, Zeroable};
use simulation::{
    FlockingConfig, Force, GpuForce, Particle, ParticleBoundary, ParticleInitialization,
    SimulationTiming, initialize_particles, initialize_particles_with,
};
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::{
    GpuContext, OffscreenError, OffscreenRenderTarget, RgbaColor,
    dispatch::dispatch_dimensions,
    flocking::{FlockingModulation, FlockingResources},
    post_process::HDR_FORMAT,
};

const PARTICLE_SHADER: &str = include_str!("../../../shaders/particles/particles.wgsl");
const MAX_FORCE_COUNT: usize = 32;

/// Per-frame inputs shared by compute and render shaders.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FrameUniforms {
    pub view_projection: [[f32; 4]; 4],
    pub frame_index: u32,
    pub particle_count: u32,
    pub delta_time: f32,
    pub simulation_time: f32,
    pub viewport_size: [f32; 2],
    pub particle_size_pixels: f32,
    pub position_scale: f32,
    pub simulation_seed: u32,
    pub force_count: u32,
    pub force_scale: f32,
    pub brightness: f32,
    pub active_particle_count: u32,
    pub confine_to_box: u32,
    pub initialization_mode: u32,
    padding: u32,
    pub initialization_params: [f32; 4],
    pub lifecycle_params: [f32; 4],
    /// near distance, far distance, size strength, brightness strength.
    pub particle_depth_response: [f32; 4],
}

const _: () = assert!(size_of::<FrameUniforms>() == 176);

#[derive(Clone, Copy, Debug)]
pub struct BenchmarkConfig {
    pub particle_size_pixels: f32,
    pub position_scale: f32,
    pub force_scale: f32,
    pub brightness: f32,
    pub hue_shift: f32,
    /// Camera-space near/far distances and size/brightness response strengths.
    pub particle_depth_response: [f32; 4],
    pub active_particle_count: Option<u32>,
    pub view_projection: Option<[[f32; 4]; 4]>,
    pub flocking: FlockingModulation,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            particle_size_pixels: 2.0,
            position_scale: 1.0,
            force_scale: 1.0,
            brightness: 1.0,
            hue_shift: 0.0,
            particle_depth_response: [1.0, 10.0, 0.0, 0.0],
            active_particle_count: None,
            view_projection: None,
            flocking: FlockingModulation::default(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ParticleRenderError {
    #[error("particle count must be greater than zero")]
    EmptyParticleSet,
    #[error(
        "particle buffers require {required} bytes, exceeding the GPU storage buffer limit of {maximum} bytes"
    )]
    BufferLimit { required: u64, maximum: u64 },
    #[error(transparent)]
    Offscreen(#[from] OffscreenError),
    #[error("GPU timing readback callback was dropped")]
    TimingCallbackDropped,
    #[error("GPU timing readback failed: {0}")]
    TimingMap(#[from] wgpu::BufferAsyncError),
    #[error("force count {count} exceeds the maximum of {maximum}")]
    TooManyForces { count: usize, maximum: usize },
}

#[derive(Clone, Copy, Debug)]
pub struct FrameTiming {
    pub cpu_prepare_ms: f64,
    pub gpu_compute_ms: Option<f64>,
    pub gpu_render_ms: Option<f64>,
}

/// Reusable GPU-resident ping-pong simulation and point rendering pipeline.
pub struct ParticleRenderer {
    particle_count: u32,
    buffers: [wgpu::Buffer; 2],
    uniform_buffer: wgpu::Buffer,
    force_buffer: wgpu::Buffer,
    bind_groups: [wgpu::BindGroup; 2],
    compute_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    source_index: usize,
    timing: Option<TimingResources>,
    seed: u64,
    initialization: ParticleInitialization,
    boundary: ParticleBoundary,
    timeline_frame: u32,
    force_count: u32,
    flocking: Option<FlockingResources>,
}

struct TimingResources {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
}

impl ParticleRenderer {
    /// Allocates and deterministically initializes GPU particle state.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty particle set or a storage allocation that
    /// exceeds the selected device's binding limit.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        context: &GpuContext,
        particle_count: u32,
        seed: u64,
    ) -> Result<Self, ParticleRenderError> {
        let particles = initialize_particles(particle_count, seed);
        Self::new_with_particles(context, &particles, seed)
    }

    /// Allocates deterministic particle state using a project-selected distribution.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty set or a storage allocation beyond the
    /// selected adapter's limits.
    pub fn new_with_initialization(
        context: &GpuContext,
        particle_count: u32,
        seed: u64,
        initialization: ParticleInitialization,
        boundary: ParticleBoundary,
    ) -> Result<Self, ParticleRenderError> {
        let particles = initialize_particles_with(particle_count, seed, initialization);
        let mut renderer = Self::new_with_particles(context, &particles, seed)?;
        renderer.initialization = initialization;
        renderer.boundary = boundary;
        Ok(renderer)
    }

    /// Allocates particle state supplied by an asset/shape target generator.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty set or a storage allocation beyond the
    /// selected adapter's limits.
    #[allow(clippy::too_many_lines)]
    pub fn new_with_particles(
        context: &GpuContext,
        particles: &[Particle],
        seed: u64,
    ) -> Result<Self, ParticleRenderError> {
        let particle_count =
            u32::try_from(particles.len()).map_err(|_| ParticleRenderError::BufferLimit {
                required: u64::MAX,
                maximum: u64::from(context.device.limits().max_storage_buffer_binding_size),
            })?;
        if particle_count == 0 {
            return Err(ParticleRenderError::EmptyParticleSet);
        }
        let required = u64::from(particle_count) * size_of::<Particle>() as u64;
        let maximum = u64::from(context.device.limits().max_storage_buffer_binding_size);
        if required > maximum {
            return Err(ParticleRenderError::BufferLimit { required, maximum });
        }
        let make_buffer = |label| {
            context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytemuck::cast_slice(particles),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                })
        };
        let buffers = [
            make_buffer("particle-buffer-a"),
            make_buffer("particle-buffer-b"),
        ];
        let uniform_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle-frame-uniforms"),
            size: size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let force_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle-forces"),
            size: (MAX_FORCE_COUNT * size_of::<GpuForce>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = context
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("particle-bind-layout"),
                entries: &[
                    storage_entry(
                        0,
                        true,
                        wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::VERTEX,
                    ),
                    storage_entry(1, false, wgpu::ShaderStages::COMPUTE),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    storage_entry(3, true, wgpu::ShaderStages::COMPUTE),
                ],
            });
        let bind_groups = [
            make_bind_group(
                &context.device,
                &layout,
                &buffers[0],
                &buffers[1],
                &uniform_buffer,
                &force_buffer,
                "particle-bind-a-b",
            ),
            make_bind_group(
                &context.device,
                &layout,
                &buffers[1],
                &buffers[0],
                &uniform_buffer,
                &force_buffer,
                "particle-bind-b-a",
            ),
        ];
        let shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("particle-shader"),
                source: wgpu::ShaderSource::Wgsl(PARTICLE_SHADER.into()),
            });
        let pipeline_layout =
            context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("particle-pipeline-layout"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                });
        let compute_pipeline =
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("particle-compute-pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some("update"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                });
        let render_pipeline =
            context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("particle-render-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vertex"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fragment"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: HDR_FORMAT,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
        Ok(Self {
            particle_count,
            buffers,
            uniform_buffer,
            force_buffer,
            bind_groups,
            compute_pipeline,
            render_pipeline,
            source_index: 0,
            timing: create_timing_resources(context),
            seed,
            initialization: ParticleInitialization::default(),
            boundary: ParticleBoundary::default(),
            timeline_frame: 0,
            force_count: 0,
            flocking: None,
        })
    }

    #[must_use]
    pub const fn particle_count(&self) -> u32 {
        self.particle_count
    }

    #[must_use]
    pub fn particle_memory_bytes(&self) -> u64 {
        u64::from(self.particle_count) * size_of::<Particle>() as u64 * 2
    }

    /// Restores both ping-pong buffers to their deterministic frame-zero state.
    pub fn reset(&mut self, context: &GpuContext) {
        let particles =
            initialize_particles_with(self.particle_count, self.seed, self.initialization);
        for buffer in &self.buffers {
            context
                .queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(&particles));
        }
        self.source_index = 0;
        self.timeline_frame = 0;
    }

    /// Uploads an ordered list of forces reused by subsequent simulation steps.
    ///
    /// # Errors
    ///
    /// Returns an error when more than 32 force entries are supplied.
    pub fn set_forces(
        &mut self,
        context: &GpuContext,
        forces: &[Force],
    ) -> Result<(), ParticleRenderError> {
        if forces.len() > MAX_FORCE_COUNT {
            return Err(ParticleRenderError::TooManyForces {
                count: forces.len(),
                maximum: MAX_FORCE_COUNT,
            });
        }
        let gpu_forces: Vec<GpuForce> = forces.iter().map(GpuForce::from).collect();
        if !gpu_forces.is_empty() {
            context
                .queue
                .write_buffer(&self.force_buffer, 0, bytemuck::cast_slice(&gpu_forces));
        }
        self.force_count =
            u32::try_from(gpu_forces.len()).map_err(|_| ParticleRenderError::TooManyForces {
                count: forces.len(),
                maximum: MAX_FORCE_COUNT,
            })?;
        Ok(())
    }

    /// Enables or disables the GPU aggregate-field flocking pass.
    pub fn set_flocking_config(&mut self, context: &GpuContext, config: Option<&FlockingConfig>) {
        self.flocking = config.cloned().and_then(|config| {
            FlockingResources::new(context, &self.buffers, self.particle_count, config)
        });
    }

    /// Deterministically seeks to and renders an offline timeline frame.
    ///
    /// Forward seeks replay fixed simulation steps. Backward seeks reset to the
    /// stored seed and replay from frame zero. Only the final color texture is
    /// read back; particle buffers remain GPU-resident.
    ///
    /// # Errors
    ///
    /// Returns an error if final texture readback fails.
    #[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
    pub fn render_timeline_frame(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        timing: SimulationTiming,
        render_config: BenchmarkConfig,
        clear: RgbaColor,
    ) -> Result<Vec<u8>, ParticleRenderError> {
        self.render_timeline_frame_gpu(context, target, frame_index, timing, render_config, clear)?;
        Ok(target.read_output(context)?)
    }

    /// Deterministically renders into the reusable post-processed GPU target
    /// without reading pixels back to the CPU.
    ///
    /// # Errors
    ///
    /// Returns an error if timeline simulation or rendering fails.
    #[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
    pub fn render_timeline_frame_gpu(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        timing: SimulationTiming,
        render_config: BenchmarkConfig,
        clear: RgbaColor,
    ) -> Result<(), ParticleRenderError> {
        let reset_history = frame_index < self.timeline_frame || frame_index == 0;
        let (initialization_mode, initialization_params, mut lifecycle_params) =
            initialization_uniforms(self.initialization);
        lifecycle_params[2] = render_config.hue_shift;
        if frame_index < self.timeline_frame {
            self.reset(context);
        }
        for frame in (self.timeline_frame + 1)..=frame_index {
            for substep in 0..timing.substeps() {
                let uniforms = FrameUniforms {
                    view_projection: render_config
                        .view_projection
                        .unwrap_or_else(|| aspect_matrix(target.dimensions())),
                    frame_index: frame,
                    particle_count: self.particle_count,
                    delta_time: timing.substep_delta(),
                    simulation_time: timing.frame_time(frame)
                        + substep as f32 * timing.substep_delta(),
                    viewport_size: [target.dimensions().0 as f32, target.dimensions().1 as f32],
                    particle_size_pixels: render_config.particle_size_pixels,
                    position_scale: render_config.position_scale,
                    simulation_seed: seed_u32(self.seed),
                    force_count: self.force_count,
                    force_scale: render_config.force_scale,
                    brightness: render_config.brightness,
                    active_particle_count: render_config
                        .active_particle_count
                        .unwrap_or(self.particle_count)
                        .min(self.particle_count),
                    confine_to_box: u32::from(self.boundary == ParticleBoundary::Box),
                    initialization_mode,
                    padding: 0,
                    initialization_params,
                    lifecycle_params,
                    particle_depth_response: render_config.particle_depth_response,
                };
                context
                    .queue
                    .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
                let active_particle_count = uniforms.active_particle_count;
                if let Some(flocking) = &self.flocking {
                    flocking.prepare(
                        context,
                        self.particle_count,
                        active_particle_count,
                        uniforms.simulation_time,
                        uniforms.delta_time,
                        uniforms.simulation_seed,
                        render_config.flocking,
                    );
                }
                let mut encoder =
                    context
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("particle-fixed-step"),
                        });
                let update_source = if let Some(flocking) = &self.flocking {
                    flocking.encode(&mut encoder, self.source_index, self.particle_count);
                    self.source_index ^ 1
                } else {
                    self.source_index
                };
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("particle-fixed-step-update"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.compute_pipeline);
                    pass.set_bind_group(0, &self.bind_groups[update_source], &[]);
                    let (groups_x, groups_y) = dispatch_dimensions(self.particle_count);
                    pass.dispatch_workgroups(groups_x, groups_y, 1);
                }
                context.queue.submit([encoder.finish()]);
                self.source_index = update_source ^ 1;
            }
        }
        self.timeline_frame = frame_index;

        let uniforms = FrameUniforms {
            view_projection: render_config
                .view_projection
                .unwrap_or_else(|| aspect_matrix(target.dimensions())),
            frame_index,
            particle_count: self.particle_count,
            delta_time: timing.substep_delta(),
            simulation_time: timing.frame_time(frame_index),
            viewport_size: [target.dimensions().0 as f32, target.dimensions().1 as f32],
            particle_size_pixels: render_config.particle_size_pixels,
            position_scale: render_config.position_scale,
            simulation_seed: seed_u32(self.seed),
            force_count: self.force_count,
            force_scale: render_config.force_scale,
            brightness: render_config.brightness,
            active_particle_count: render_config
                .active_particle_count
                .unwrap_or(self.particle_count)
                .min(self.particle_count),
            confine_to_box: u32::from(self.boundary == ParticleBoundary::Box),
            initialization_mode,
            padding: 0,
            initialization_params,
            lifecycle_params,
            particle_depth_response: render_config.particle_depth_response,
        };
        context
            .queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let view = target.view();
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("particle-timeline-render"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particle-timeline-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear.into()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.render_pipeline);
            pass.set_bind_group(0, &self.bind_groups[self.source_index], &[]);
            pass.draw(0..self.particle_count.saturating_mul(6), 0..1);
        }
        target.encode_post_process(&mut encoder, reset_history);
        context.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Deterministically seeks to a timeline frame and saves it as PNG.
    ///
    /// # Errors
    ///
    /// Returns an error if rendering, readback, or PNG encoding fails.
    #[allow(clippy::too_many_arguments)]
    pub fn save_timeline_frame_png(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        timing: SimulationTiming,
        render_config: BenchmarkConfig,
        clear: RgbaColor,
        path: impl AsRef<Path>,
    ) -> Result<(), ParticleRenderError> {
        let pixels =
            self.render_timeline_frame(context, target, frame_index, timing, render_config, clear)?;
        Ok(target.save_png(&pixels, path.as_ref())?)
    }

    /// Measures CPU frame preparation and GPU compute/render work without image readback.
    ///
    /// # Errors
    ///
    /// Returns an error if timestamp results cannot be mapped. GPU fields are
    /// `None` on adapters without timestamp-query support.
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    pub fn benchmark_frame(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        fps: f32,
        config: BenchmarkConfig,
    ) -> Result<FrameTiming, ParticleRenderError> {
        let started = Instant::now();
        let (initialization_mode, initialization_params, mut lifecycle_params) =
            initialization_uniforms(self.initialization);
        lifecycle_params[2] = config.hue_shift;
        let uniforms = FrameUniforms {
            view_projection: config
                .view_projection
                .unwrap_or_else(|| aspect_matrix(target.dimensions())),
            frame_index,
            particle_count: self.particle_count,
            delta_time: 1.0 / fps,
            simulation_time: frame_index as f32 / fps,
            viewport_size: [target.dimensions().0 as f32, target.dimensions().1 as f32],
            particle_size_pixels: config.particle_size_pixels,
            position_scale: config.position_scale,
            simulation_seed: seed_u32(self.seed),
            force_count: self.force_count,
            force_scale: config.force_scale,
            brightness: config.brightness,
            active_particle_count: config
                .active_particle_count
                .unwrap_or(self.particle_count)
                .min(self.particle_count),
            confine_to_box: u32::from(self.boundary == ParticleBoundary::Box),
            initialization_mode,
            padding: 0,
            initialization_params,
            lifecycle_params,
            particle_depth_response: config.particle_depth_response,
        };
        context
            .queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        if let Some(flocking) = &self.flocking {
            flocking.prepare(
                context,
                self.particle_count,
                uniforms.active_particle_count,
                uniforms.simulation_time,
                uniforms.delta_time,
                uniforms.simulation_seed,
                config.flocking,
            );
        }
        let view = target.view();
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("particle-benchmark-frame"),
            });
        let update_source = if let Some(flocking) = &self.flocking {
            flocking.encode(&mut encoder, self.source_index, self.particle_count);
            self.source_index ^ 1
        } else {
            self.source_index
        };
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("particle-benchmark-compute"),
                timestamp_writes: self.timing.as_ref().map(|timing| {
                    wgpu::ComputePassTimestampWrites {
                        query_set: &timing.queries,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }
                }),
            });
            pass.set_pipeline(&self.compute_pipeline);
            pass.set_bind_group(0, &self.bind_groups[update_source], &[]);
            let (groups_x, groups_y) = dispatch_dimensions(self.particle_count);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particle-benchmark-render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.timing.as_ref().map(|timing| {
                    wgpu::RenderPassTimestampWrites {
                        query_set: &timing.queries,
                        beginning_of_pass_write_index: Some(2),
                        end_of_pass_write_index: Some(3),
                    }
                }),
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.render_pipeline);
            pass.set_bind_group(0, &self.bind_groups[update_source ^ 1], &[]);
            pass.draw(0..self.particle_count.saturating_mul(6), 0..1);
        }
        if let Some(timing) = &self.timing {
            encoder.resolve_query_set(&timing.queries, 0..4, &timing.resolve, 0);
            encoder.copy_buffer_to_buffer(&timing.resolve, 0, &timing.readback, 0, 32);
        }
        let cpu_prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
        context.queue.submit([encoder.finish()]);
        self.source_index = update_source ^ 1;
        let (gpu_compute_ms, gpu_render_ms) = if let Some(timing) = &self.timing {
            let values = read_timestamps(context, &timing.readback)?;
            let period_ms = f64::from(context.queue.get_timestamp_period()) / 1_000_000.0;
            (
                (values[1] - values[0]) as f64 * period_ms,
                (values[3] - values[2]) as f64 * period_ms,
            )
        } else {
            (0.0, 0.0)
        };
        Ok(FrameTiming {
            cpu_prepare_ms,
            gpu_compute_ms: self.timing.as_ref().map(|_| gpu_compute_ms),
            gpu_render_ms: self.timing.as_ref().map(|_| gpu_render_ms),
        })
    }

    /// Advances one fixed frame and renders the resulting GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the completed texture cannot be read back.
    #[allow(clippy::cast_precision_loss)]
    pub fn render_frame(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        fps: f32,
        clear: RgbaColor,
    ) -> Result<Vec<u8>, ParticleRenderError> {
        let (initialization_mode, initialization_params, lifecycle_params) =
            initialization_uniforms(self.initialization);
        let uniforms = FrameUniforms {
            view_projection: aspect_matrix(target.dimensions()),
            frame_index,
            particle_count: self.particle_count,
            delta_time: 1.0 / fps,
            simulation_time: frame_index as f32 / fps,
            viewport_size: [target.dimensions().0 as f32, target.dimensions().1 as f32],
            particle_size_pixels: 2.0,
            position_scale: 1.0,
            simulation_seed: seed_u32(self.seed),
            force_count: self.force_count,
            force_scale: 1.0,
            brightness: 1.0,
            active_particle_count: self.particle_count,
            confine_to_box: u32::from(self.boundary == ParticleBoundary::Box),
            initialization_mode,
            padding: 0,
            initialization_params,
            lifecycle_params,
            particle_depth_response: [1.0, 10.0, 0.0, 0.0],
        };
        context
            .queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        if let Some(flocking) = &self.flocking {
            flocking.prepare(
                context,
                self.particle_count,
                self.particle_count,
                uniforms.simulation_time,
                uniforms.delta_time,
                uniforms.simulation_seed,
                FlockingModulation::default(),
            );
        }
        let view = target.view();
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("particle-frame"),
            });
        let update_source = if let Some(flocking) = &self.flocking {
            flocking.encode(&mut encoder, self.source_index, self.particle_count);
            self.source_index ^ 1
        } else {
            self.source_index
        };
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("particle-update"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.compute_pipeline);
            pass.set_bind_group(0, &self.bind_groups[update_source], &[]);
            let (groups_x, groups_y) = dispatch_dimensions(self.particle_count);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particle-render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear.into()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.render_pipeline);
            pass.set_bind_group(0, &self.bind_groups[update_source ^ 1], &[]);
            pass.draw(0..self.particle_count.saturating_mul(6), 0..1);
        }
        target.encode_readback(&mut encoder, frame_index == 0);
        context.queue.submit([encoder.finish()]);
        self.source_index = update_source ^ 1;
        Ok(target.read_pixels(context)?)
    }

    /// Advances a frame and encodes its final pixels as PNG.
    ///
    /// # Errors
    ///
    /// Returns an error if rendering, readback, file creation, or PNG encoding fails.
    pub fn save_frame_png(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        fps: f32,
        clear: RgbaColor,
        path: impl AsRef<Path>,
    ) -> Result<(), ParticleRenderError> {
        let pixels = self.render_frame(context, target, frame_index, fps, clear)?;
        Ok(target.save_png(&pixels, path.as_ref())?)
    }
}

fn storage_entry(
    binding: u32,
    read_only: bool,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn seed_u32(seed: u64) -> u32 {
    let bytes = seed.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn initialization_uniforms(initialization: ParticleInitialization) -> (u32, [f32; 4], [f32; 4]) {
    match initialization {
        ParticleInitialization::Volume => (0, [0.0; 4], [0.0; 4]),
        ParticleInitialization::GalacticDisk {
            radius,
            thickness,
            lifetime_seconds,
            spawn_spread_seconds,
            lifetime_variation,
        } => (
            1,
            [radius, thickness, lifetime_seconds, 0.0],
            [spawn_spread_seconds, lifetime_variation, 0.0, 0.0],
        ),
    }
}

fn create_timing_resources(context: &GpuContext) -> Option<TimingResources> {
    if !context
        .device
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY)
    {
        return None;
    }
    let queries = context.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("particle-timing-queries"),
        ty: wgpu::QueryType::Timestamp,
        count: 4,
    });
    let resolve = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle-timing-resolve"),
        size: 32,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle-timing-readback"),
        size: 32,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    Some(TimingResources {
        queries,
        resolve,
        readback,
    })
}

fn read_timestamps(
    context: &GpuContext,
    buffer: &wgpu::Buffer,
) -> Result<[u64; 4], ParticleRenderError> {
    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    let _ = context.device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|_| ParticleRenderError::TimingCallbackDropped)??;
    let mapped = slice.get_mapped_range();
    let mut values = [0_u64; 4];
    values.copy_from_slice(bytemuck::cast_slice(&mapped));
    drop(mapped);
    buffer.unmap();
    Ok(values)
}

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::Buffer,
    destination: &wgpu::Buffer,
    uniforms: &wgpu::Buffer,
    forces: &wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: source.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: forces.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: destination.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniforms.as_entire_binding(),
            },
        ],
    })
}

#[allow(clippy::cast_precision_loss)]
fn aspect_matrix((width, height): (u32, u32)) -> [[f32; 4]; 4] {
    let aspect = width as f32 / height as f32;
    let x_scale = if aspect > 1.0 { 1.0 / aspect } else { 1.0 };
    let y_scale = if aspect < 1.0 { aspect } else { 1.0 };
    [
        [x_scale, 0.0, 0.0, 0.0],
        [0.0, y_scale, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
