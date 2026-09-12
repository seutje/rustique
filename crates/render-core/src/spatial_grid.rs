use std::{path::Path, sync::mpsc, time::Instant};

use bytemuck::{Pod, Zeroable};
use simulation::initialize_particles;
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::GpuContext;

const SHADER: &str = include_str!("../../../shaders/spatial/uniform_grid.wgsl");

#[derive(Clone, Copy, Debug)]
pub struct SpatialGridConfig {
    pub cells_per_axis: u32,
    pub cell_capacity: u32,
    pub max_neighbors: u32,
    pub world_min: [f32; 3],
    pub world_max: [f32; 3],
}

impl Default for SpatialGridConfig {
    fn default() -> Self {
        Self {
            cells_per_axis: 32,
            cell_capacity: 64,
            max_neighbors: 128,
            world_min: [-1.0; 3],
            world_max: [1.0; 3],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SpatialGridStats {
    pub particle_count: u32,
    pub cell_count: u32,
    pub occupied_cells: u32,
    pub maximum_cell_occupancy: u32,
    pub average_neighbors: f64,
    pub maximum_neighbors: u32,
    pub overflowed_particles: u32,
    pub gpu_memory_bytes: u64,
    pub elapsed_ms: f64,
}

#[derive(Debug, Error)]
pub enum SpatialGridError {
    #[error("spatial grid values and particle count must be greater than zero")]
    ZeroValue,
    #[error("spatial grid bounds must be finite and world_max must exceed world_min on every axis")]
    InvalidBounds,
    #[error(
        "spatial grid allocation requires {required} bytes, exceeding the storage binding limit of {maximum} bytes"
    )]
    BufferLimit { required: u64, maximum: u64 },
    #[error("GPU spatial-grid readback callback was dropped")]
    CallbackDropped,
    #[error("GPU spatial-grid readback failed: {0}")]
    Map(#[from] wgpu::BufferAsyncError),
    #[error("failed to create debug PNG {path}: {source}")]
    DebugImage {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to encode debug PNG: {0}")]
    Png(#[from] png::EncodingError),
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Position {
    value: [f32; 4],
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
}

pub struct SpatialGrid {
    config: SpatialGridConfig,
    particle_count: u32,
    cell_count: u32,
    counts: wgpu::Buffer,
    neighbors: wgpu::Buffer,
    overflow: wgpu::Buffer,
    readback: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    clear_pipeline: wgpu::ComputePipeline,
    build_pipeline: wgpu::ComputePipeline,
    query_pipeline: wgpu::ComputePipeline,
    memory_bytes: u64,
}

impl SpatialGrid {
    /// Allocates a GPU-resident uniform grid and deterministic test population.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid dimensions or bounds, arithmetic overflow,
    /// or a grid entry allocation beyond the adapter's binding limit.
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    pub fn new(
        context: &GpuContext,
        particle_count: u32,
        seed: u64,
        config: SpatialGridConfig,
    ) -> Result<Self, SpatialGridError> {
        if particle_count == 0
            || config.cells_per_axis == 0
            || config.cell_capacity == 0
            || config.max_neighbors == 0
        {
            return Err(SpatialGridError::ZeroValue);
        }
        if config
            .world_min
            .iter()
            .chain(config.world_max.iter())
            .any(|v| !v.is_finite())
            || (0..3).any(|i| config.world_max[i] <= config.world_min[i])
        {
            return Err(SpatialGridError::InvalidBounds);
        }
        let cell_count =
            config
                .cells_per_axis
                .checked_pow(3)
                .ok_or(SpatialGridError::BufferLimit {
                    required: u64::MAX,
                    maximum: u64::from(context.device.limits().max_storage_buffer_binding_size),
                })?;
        let entries_bytes = u64::from(cell_count) * u64::from(config.cell_capacity) * 4;
        let maximum = u64::from(context.device.limits().max_storage_buffer_binding_size);
        if entries_bytes > maximum {
            return Err(SpatialGridError::BufferLimit {
                required: entries_bytes,
                maximum,
            });
        }
        let particles = initialize_particles(particle_count, seed);
        let positions: Vec<Position> = particles
            .iter()
            .map(|p| Position {
                value: p.position_age,
            })
            .collect();
        let position_bytes = u64::from(particle_count) * 16;
        let counts_bytes = u64::from(cell_count) * 4;
        let neighbors_bytes = u64::from(particle_count) * 4;
        let positions = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("spatial-grid-positions"),
                contents: bytemuck::cast_slice(&positions),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let buffer = |label, size| {
            context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let counts = buffer("spatial-grid-counts", counts_bytes);
        let entries = buffer("spatial-grid-entries", entries_bytes);
        let neighbors = buffer("spatial-grid-neighbors", neighbors_bytes);
        let overflow = buffer("spatial-grid-overflow", 4);
        let readback_size = counts_bytes + neighbors_bytes + 4;
        let readback = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spatial-grid-readback"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let extent = config.world_max[0] - config.world_min[0];
        let params = Params {
            particle_count,
            cells_per_axis: config.cells_per_axis,
            cell_capacity: config.cell_capacity,
            max_neighbors: config.max_neighbors,
            world_min: config.world_min,
            cell_size: extent / config.cells_per_axis as f32,
        };
        let params = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("spatial-grid-params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let module = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("spatial-grid-shader"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let layout = context
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("spatial-grid-layout"),
                entries: &[
                    storage(0, true),
                    storage(1, false),
                    storage(2, false),
                    storage(3, false),
                    storage(4, false),
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
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
        let bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("spatial-grid-bind-group"),
                layout: &layout,
                entries: &[
                    entry(0, &positions),
                    entry(1, &counts),
                    entry(2, &entries),
                    entry(3, &neighbors),
                    entry(4, &overflow),
                    entry(5, &params),
                ],
            });
        let pipeline_layout =
            context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("spatial-grid-pipeline-layout"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                });
        let pipeline = |name| {
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(name),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(name),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                })
        };
        Ok(Self {
            config,
            particle_count,
            cell_count,
            counts,
            neighbors,
            overflow,
            readback,
            bind_group,
            clear_pipeline: pipeline("clear"),
            build_pipeline: pipeline("build"),
            query_pipeline: pipeline("query"),
            memory_bytes: position_bytes + counts_bytes + entries_bytes + neighbors_bytes + 4,
        })
    }

    /// Builds the grid, queries neighbors, and reads diagnostic counters back.
    ///
    /// # Errors
    ///
    /// Returns an error when the diagnostic GPU readback fails.
    #[allow(clippy::cast_precision_loss)]
    pub fn run(
        &self,
        context: &GpuContext,
    ) -> Result<(SpatialGridStats, Vec<u32>), SpatialGridError> {
        let started = Instant::now();
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("spatial-grid-run"),
            });
        for (pipeline, items) in [
            (
                &self.clear_pipeline,
                self.cell_count.max(self.particle_count),
            ),
            (&self.build_pipeline, self.particle_count),
            (&self.query_pipeline, self.particle_count),
        ] {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("spatial-grid-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(items.div_ceil(256), 1, 1);
        }
        let counts_bytes = u64::from(self.cell_count) * 4;
        let neighbors_bytes = u64::from(self.particle_count) * 4;
        encoder.copy_buffer_to_buffer(&self.counts, 0, &self.readback, 0, counts_bytes);
        encoder.copy_buffer_to_buffer(
            &self.neighbors,
            0,
            &self.readback,
            counts_bytes,
            neighbors_bytes,
        );
        encoder.copy_buffer_to_buffer(
            &self.overflow,
            0,
            &self.readback,
            counts_bytes + neighbors_bytes,
            4,
        );
        context.queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = context.device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .map_err(|_| SpatialGridError::CallbackDropped)??;
        let mapped = slice.get_mapped_range();
        let words: &[u32] = bytemuck::cast_slice(&mapped);
        let cells = words[..self.cell_count as usize].to_vec();
        let neighbor_values =
            &words[self.cell_count as usize..(self.cell_count + self.particle_count) as usize];
        let overflowed_particles = words[(self.cell_count + self.particle_count) as usize];
        let occupied_cells =
            u32::try_from(cells.iter().filter(|&&v| v != 0).count()).unwrap_or(u32::MAX);
        let maximum_cell_occupancy = cells.iter().copied().max().unwrap_or(0);
        let maximum_neighbors = neighbor_values.iter().copied().max().unwrap_or(0);
        let average_neighbors = neighbor_values.iter().map(|&v| u64::from(v)).sum::<u64>() as f64
            / f64::from(self.particle_count);
        drop(mapped);
        self.readback.unmap();
        Ok((
            SpatialGridStats {
                particle_count: self.particle_count,
                cell_count: self.cell_count,
                occupied_cells,
                maximum_cell_occupancy,
                average_neighbors,
                maximum_neighbors,
                overflowed_particles,
                gpu_memory_bytes: self.memory_bytes,
                elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            },
            cells,
        ))
    }

    /// Saves a Z-projected cell occupancy heatmap.
    ///
    /// # Errors
    ///
    /// Returns an error when the output file cannot be created or encoded.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss
    )]
    pub fn save_debug_png(
        &self,
        cell_counts: &[u32],
        path: impl AsRef<Path>,
    ) -> Result<(), SpatialGridError> {
        let n = self.config.cells_per_axis as usize;
        let mut projected = vec![0u32; n * n];
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    projected[y * n + x] =
                        projected[y * n + x].saturating_add(cell_counts[x + n * (y + n * z)]);
                }
            }
        }
        let maximum = projected.iter().copied().max().unwrap_or(1).max(1) as f32;
        let mut pixels = Vec::with_capacity(n * n * 4);
        for value in projected {
            let heat = (value as f32 / maximum).sqrt();
            pixels.extend_from_slice(&[
                (255.0 * heat) as u8,
                (180.0 * heat) as u8,
                (255.0 * (1.0 - heat)) as u8,
                255,
            ]);
        }
        let path = path.as_ref();
        let file = std::fs::File::create(path).map_err(|source| SpatialGridError::DebugImage {
            path: path.display().to_string(),
            source,
        })?;
        let mut encoder =
            png::Encoder::new(file, self.config.cells_per_axis, self.config.cells_per_axis);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;
        Ok(())
    }
}

fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
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
fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_grid_has_valid_bounds() {
        let config = SpatialGridConfig::default();
        assert!(config.world_max[0] > config.world_min[0]);
        assert!(config.cell_capacity > 0);
    }
}
