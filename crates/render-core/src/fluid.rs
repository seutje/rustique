use crate::{GpuContext, OffscreenError, OffscreenRenderTarget, RgbaColor};
use bytemuck::{Pod, Zeroable};
use simulation::initialize_particles;
use std::{path::Path, sync::mpsc, time::Instant};
use thiserror::Error;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("../../../shaders/fluid/sph.wgsl");
const RENDER_SHADER: &str = include_str!("../../../shaders/fluid/render.wgsl");

#[derive(Clone, Copy, Debug)]
pub struct FluidConfig {
    pub cells_per_axis: u32,
    pub cell_capacity: u32,
    pub max_neighbors: u32,
    pub rest_density: f32,
    pub pressure: f32,
    pub viscosity: f32,
    pub cohesion: f32,
    pub audio_pressure: f32,
    pub audio_turbulence: f32,
    pub dt: f32,
    pub particle_size: f32,
}
impl Default for FluidConfig {
    fn default() -> Self {
        Self {
            cells_per_axis: 32,
            cell_capacity: 64,
            max_neighbors: 96,
            rest_density: 8.0,
            pressure: 0.45,
            viscosity: 0.18,
            cohesion: 0.35,
            audio_pressure: 0.0,
            audio_turbulence: 0.0,
            dt: 1.0 / 120.0,
            particle_size: 0.006,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct FluidFrameStats {
    pub particle_count: u32,
    pub average_density: f64,
    pub maximum_density: f32,
    pub gpu_memory_bytes: u64,
    pub elapsed_ms: f64,
}
#[derive(Debug, Error)]
pub enum FluidError {
    #[error("fluid particle/grid values must be greater than zero")]
    ZeroValue,
    #[error("fluid GPU buffer requires {required} bytes, exceeding limit {maximum}")]
    BufferLimit { required: u64, maximum: u64 },
    #[error("fluid readback callback was dropped")]
    CallbackDropped,
    #[error("fluid GPU readback failed: {0}")]
    Map(#[from] wgpu::BufferAsyncError),
    #[error(transparent)]
    Offscreen(#[from] OffscreenError),
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FluidParticle {
    position: [f32; 4],
    velocity: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    particle_count: u32,
    cells_per_axis: u32,
    cell_capacity: u32,
    max_neighbors: u32,
    world_min: [f32; 3],
    cell_size: f32,
    dt: f32,
    rest_density: f32,
    pressure: f32,
    viscosity: f32,
    cohesion: f32,
    audio_pressure: f32,
    audio_turbulence: f32,
    particle_size: f32,
    view_aspect: f32,
    pad: [f32; 3],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RenderParams {
    rest_density: f32,
    particle_size: f32,
    view_aspect: f32,
    padding: f32,
}
pub struct FluidRenderer {
    count: u32,
    cells: u32,
    _buffers: [wgpu::Buffer; 2],
    _counts: wgpu::Buffer,
    densities: wgpu::Buffer,
    readback: wgpu::Buffer,
    _params: wgpu::Buffer,
    groups: [wgpu::BindGroup; 2],
    render_groups: [wgpu::BindGroup; 2],
    clear: wgpu::ComputePipeline,
    build: wgpu::ComputePipeline,
    density: wgpu::ComputePipeline,
    solve: wgpu::ComputePipeline,
    render: wgpu::RenderPipeline,
    source: usize,
    memory: u64,
}
impl FluidRenderer {
    /// Creates the GPU-resident SPH buffers and pipelines.
    ///
    /// # Errors
    ///
    /// Returns an error for zero dimensions or when a grid buffer exceeds the
    /// adapter's storage-binding limit.
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::too_many_lines)]
    pub fn new(
        ctx: &GpuContext,
        count: u32,
        seed: u64,
        config: FluidConfig,
        width: u32,
        height: u32,
    ) -> Result<Self, FluidError> {
        if count == 0
            || config.cells_per_axis == 0
            || config.cell_capacity == 0
            || config.max_neighbors == 0
        {
            return Err(FluidError::ZeroValue);
        }
        let cells = config
            .cells_per_axis
            .checked_pow(3)
            .ok_or(FluidError::ZeroValue)?;
        let entry_bytes = u64::from(cells) * u64::from(config.cell_capacity) * 4;
        let max = u64::from(ctx.device.limits().max_storage_buffer_binding_size);
        if entry_bytes > max {
            return Err(FluidError::BufferLimit {
                required: entry_bytes,
                maximum: max,
            });
        }
        let initial: Vec<_> = initialize_particles(count, seed)
            .into_iter()
            .map(|p| FluidParticle {
                position: [
                    p.position_age[0] * 0.55,
                    p.position_age[1] * 0.55,
                    p.position_age[2] * 0.55,
                    0.0,
                ],
                velocity: [0.0; 4],
            })
            .collect();
        let particle_bytes = u64::from(count) * 32;
        let make_particles = |label| {
            ctx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytemuck::cast_slice(&initial),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                })
        };
        let buffers = [make_particles("fluid-a"), make_particles("fluid-b")];
        let storage = |label, size| {
            ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let counts = storage("fluid-counts", u64::from(cells) * 4);
        let entries = storage("fluid-entries", entry_bytes);
        let densities = storage("fluid-densities", u64::from(count) * 4);
        let readback = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fluid-density-readback"),
            size: u64::from(count) * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let params = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fluid-params"),
                contents: bytemuck::bytes_of(&Params {
                    particle_count: count,
                    cells_per_axis: config.cells_per_axis,
                    cell_capacity: config.cell_capacity,
                    max_neighbors: config.max_neighbors,
                    world_min: [-1.0; 3],
                    cell_size: 2.0 / config.cells_per_axis as f32,
                    dt: config.dt,
                    rest_density: config.rest_density,
                    pressure: config.pressure,
                    viscosity: config.viscosity,
                    cohesion: config.cohesion,
                    audio_pressure: config.audio_pressure,
                    audio_turbulence: config.audio_turbulence,
                    particle_size: config.particle_size,
                    view_aspect: width as f32 / height as f32,
                    pad: [0.0; 3],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("fluid-sph"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("fluid-layout"),
                entries: &[
                    storage_entry(0, true),
                    storage_entry(1, false),
                    storage_entry(2, false),
                    storage_entry(3, false),
                    storage_entry(4, false),
                    uniform_entry(5),
                ],
            });
        let group = |src: &wgpu::Buffer, dst: &wgpu::Buffer, label| {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &layout,
                entries: &[
                    entry(0, src),
                    entry(1, dst),
                    entry(2, &counts),
                    entry(3, &entries),
                    entry(4, &densities),
                    entry(5, &params),
                ],
            })
        };
        let groups = [
            group(&buffers[0], &buffers[1], "fluid-ab"),
            group(&buffers[1], &buffers[0], "fluid-ba"),
        ];
        let pl = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("fluid-pipeline-layout"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let cp = |name| {
            ctx.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name),
                    layout: Some(&pl),
                    module: &module,
                    entry_point: Some(name),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                })
        };
        let render_module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("fluid-render-shader"),
                source: wgpu::ShaderSource::Wgsl(RENDER_SHADER.into()),
            });
        let render_layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("fluid-render-layout"),
                entries: &[render_storage_entry(0), render_uniform_entry(1)],
            });
        let render_params = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("fluid-render-params"),
                contents: bytemuck::bytes_of(&RenderParams {
                    rest_density: config.rest_density,
                    particle_size: config.particle_size,
                    view_aspect: width as f32 / height as f32,
                    padding: 0.0,
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let render_group = |buffer: &wgpu::Buffer, label| {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &render_layout,
                entries: &[entry(0, buffer), entry(1, &render_params)],
            })
        };
        let render_groups = [
            render_group(&buffers[0], "fluid-render-a"),
            render_group(&buffers[1], "fluid-render-b"),
        ];
        let render_pl = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("fluid-render-pipeline-layout"),
                bind_group_layouts: &[&render_layout],
                push_constant_ranges: &[],
            });
        let render = ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("fluid-render"),
                layout: Some(&render_pl),
                vertex: wgpu::VertexState {
                    module: &render_module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &render_module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
        Ok(Self {
            count,
            cells,
            _buffers: buffers,
            _counts: counts,
            densities,
            readback,
            _params: params,
            groups,
            render_groups,
            clear: cp("clear"),
            build: cp("build"),
            density: cp("density"),
            solve: cp("solve"),
            render,
            source: 0,
            memory: particle_bytes * 2 + u64::from(cells) * 4 + entry_bytes + u64::from(count) * 4,
        })
    }
    /// Advances one fixed step, renders it, and returns density diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an error when GPU density or pixel readback fails.
    pub fn render_frame(
        &mut self,
        ctx: &GpuContext,
        target: &OffscreenRenderTarget,
        clear: RgbaColor,
    ) -> Result<(Vec<u8>, FluidFrameStats), FluidError> {
        let start = Instant::now();
        let mut enc = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fluid-frame"),
            });
        for (p, n) in [
            (&self.clear, self.cells.max(self.count)),
            (&self.build, self.count),
            (&self.density, self.count),
            (&self.solve, self.count),
        ] {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fluid-compute"),
                timestamp_writes: None,
            });
            pass.set_pipeline(p);
            pass.set_bind_group(0, &self.groups[self.source], &[]);
            pass.dispatch_workgroups(n.div_ceil(256), 1, 1);
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fluid-render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
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
            pass.set_pipeline(&self.render);
            pass.set_bind_group(0, &self.render_groups[self.source ^ 1], &[]);
            pass.draw(0..self.count.saturating_mul(6), 0..1);
        }
        enc.copy_buffer_to_buffer(
            &self.densities,
            0,
            &self.readback,
            0,
            u64::from(self.count) * 4,
        );
        target.encode_readback(&mut enc, false);
        ctx.queue.submit([enc.finish()]);
        self.source ^= 1;
        let values = read(ctx, &self.readback)?;
        let avg = values.iter().map(|v| f64::from(*v)).sum::<f64>() / f64::from(self.count);
        let maximum = values.iter().copied().fold(0.0, f32::max);
        Ok((
            target.read_pixels(ctx)?,
            FluidFrameStats {
                particle_count: self.count,
                average_density: avg,
                maximum_density: maximum,
                gpu_memory_bytes: self.memory,
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
            },
        ))
    }
    /// Advances at least one fixed step and saves the last frame as PNG.
    ///
    /// # Errors
    ///
    /// Returns an error when simulation, readback, or PNG encoding fails.
    pub fn save_png(
        &mut self,
        ctx: &GpuContext,
        target: &OffscreenRenderTarget,
        frames: u32,
        path: impl AsRef<Path>,
    ) -> Result<FluidFrameStats, FluidError> {
        let mut result = self.render_frame(ctx, target, RgbaColor::new(0.0, 0.01, 0.0, 1.0))?;
        for _ in 1..frames.max(1) {
            result = self.render_frame(ctx, target, RgbaColor::new(0.0, 0.01, 0.0, 1.0))?;
        }
        let (pixels, stats) = result;
        target.save_png(&pixels, path.as_ref())?;
        Ok(stats)
    }
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
fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn render_storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn render_uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn entry(binding: u32, b: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: b.as_entire_binding(),
    }
}
fn read(ctx: &GpuContext, b: &wgpu::Buffer) -> Result<Vec<f32>, FluidError> {
    let s = b.slice(..);
    let (tx, rx) = mpsc::channel();
    s.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = ctx.device.poll(wgpu::Maintain::Wait);
    rx.recv().map_err(|_| FluidError::CallbackDropped)??;
    let m = s.get_mapped_range();
    let v = bytemuck::cast_slice(&m).to_vec();
    drop(m);
    b.unmap();
    Ok(v)
}
