//! GPU waterfall: dB rows live in a ring texture, colourised in a fragment shader.
//!
//! The CPU path composes, stretches and colourises 360 rows on every viewport
//! change — pan, zoom or resize — because the texture it uploads is already in
//! screen space. Here the texture holds raw dB at full span, so pan and zoom are
//! uniform updates and cost nothing: only a genuinely new FFT row is uploaded,
//! and only one row of `f32`.
//!
//! Horizontal resampling is peak-hold, not bilinear. That matters for CW: a
//! carrier can occupy a single bin, and averaging it with its neighbours when
//! zoomed out dims exactly the signal the operator is hunting. The shader takes
//! the max across the texels each output pixel covers, matching
//! `downsample_row_peak` on the CPU side.

use std::num::NonZeroU64;

use eframe::egui_wgpu::{self, wgpu};
use eframe::egui::{PaintCallbackInfo, Rect};

/// Rows retained in the ring texture (matches `engine::WATERFALL_ROWS`).
pub const RING_ROWS: usize = 360;

/// Peak-hold taps per output pixel. Above this the extra samples stop changing
/// the result on any realistic zoom-out and only cost fill rate.
pub const MAX_TAPS: u32 = 32;

/// Values below this read as "no data" and paint the floor colour.
pub const DB_FLOOR: f32 = -200.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WaterfallUniforms {
    /// Left edge of the view in texture u.
    pub u0: f32,
    /// Right edge of the view in texture u.
    pub u1: f32,
    pub ref_db: f32,
    pub range_db: f32,
    /// Ring write head, in rows.
    pub row_head: f32,
    pub row_count: f32,
    /// Half the u-width one output pixel covers, for peak-hold.
    pub half_span_u: f32,
    /// Peak-hold taps actually used this frame (1..=MAX_TAPS).
    pub taps: f32,
}

// Safety: plain old data, no padding gaps (8 x f32 = 32 bytes, 16-byte aligned).
unsafe impl bytemuck_lite::Pod for WaterfallUniforms {}

/// Minimal local stand-in for `bytemuck` so this does not add a dependency.
mod bytemuck_lite {
    /// # Safety
    /// Implementors must be plain-old-data with no padding and no invalid bit patterns.
    pub unsafe trait Pod: Copy {}

    /// Reinterpret a slice of POD values as bytes.
    pub fn bytes_of_slice<T: Pod>(values: &[T]) -> &[u8] {
        // Safety: `T: Pod` guarantees every byte is initialized and readable.
        unsafe {
            std::slice::from_raw_parts(
                values.as_ptr().cast::<u8>(),
                std::mem::size_of_val(values),
            )
        }
    }

    unsafe impl Pod for f32 {}

    pub fn bytes_of<T: Pod>(value: &T) -> &[u8] {
        // Safety: `T: Pod` guarantees every byte is initialized and readable.
        unsafe {
            std::slice::from_raw_parts(
                (value as *const T).cast::<u8>(),
                std::mem::size_of::<T>(),
            )
        }
    }
}

/// Compute the uniform block for one frame.
///
/// `u0`/`u1` are the visible window in storage-texture coordinates; `plot_px` is
/// the on-screen width so peak-hold can cover exactly the texels each pixel spans.
pub fn uniforms_for(
    u0: f64,
    u1: f64,
    row_head: usize,
    row_width: usize,
    plot_px: f32,
    ref_db: f32,
    range_db: f32,
) -> WaterfallUniforms {
    let span = (u1 - u0).abs().max(f64::MIN_POSITIVE);
    let px = plot_px.max(1.0) as f64;
    let texels_per_pixel = span * row_width.max(1) as f64 / px;
    let taps = texels_per_pixel.ceil().clamp(1.0, MAX_TAPS as f64);
    WaterfallUniforms {
        u0: u0 as f32,
        u1: u1 as f32,
        ref_db,
        // A zero range would divide by zero in the shader.
        range_db: if range_db.abs() < f32::EPSILON { 1.0 } else { range_db },
        row_head: (row_head % RING_ROWS.max(1)) as f32,
        row_count: RING_ROWS as f32,
        half_span_u: (span / px / 2.0) as f32,
        taps: taps as f32,
    }
}

const SHADER: &str = r#"
struct Uniforms {
    u0: f32,
    u1: f32,
    ref_db: f32,
    range_db: f32,
    row_head: f32,
    row_count: f32,
    half_span_u: f32,
    taps: f32,
};

@group(0) @binding(0) var<uniform> U: Uniforms;
@group(0) @binding(1) var db_tex: texture_2d<f32>;
@group(0) @binding(2) var db_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Fullscreen triangle over the callback's viewport.
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    var out: VsOut;
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    out.uv = vec2<f32>(x, y);
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

// Waterfall ramp: deep violet -> cyan -> amber -> white.
// Must track `dsp::colormap::STOPS`.
fn ramp(t_in: f32) -> vec3<f32> {
    let stops = array<vec3<f32>, 8>(
        vec3<f32>(0.03, 0.02, 0.08),
        vec3<f32>(0.08, 0.05, 0.28),
        vec3<f32>(0.05, 0.22, 0.55),
        vec3<f32>(0.04, 0.55, 0.72),
        vec3<f32>(0.15, 0.78, 0.55),
        vec3<f32>(0.85, 0.72, 0.12),
        vec3<f32>(0.98, 0.88, 0.35),
        vec3<f32>(1.00, 1.00, 0.98),
    );
    let t = clamp(t_in, 0.0, 1.0) * 7.0;
    let i = i32(floor(t));
    let j = min(i + 1, 7);
    let f = t - floor(t);
    return mix(stops[i], stops[j], f);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Newest row at the top, walking backwards through the ring.
    let s = floor(clamp(in.uv.y, 0.0, 0.9999) * U.row_count);
    let tex_row = (U.row_head + U.row_count - 1.0 - s) % U.row_count;
    let v = (tex_row + 0.5) / U.row_count;

    let u_center = U.u0 + in.uv.x * (U.u1 - U.u0);

    // Peak-hold across the texels this pixel covers: a one-bin CW carrier must
    // survive zoom-out rather than being averaged into the noise floor.
    let n = max(1, i32(U.taps));
    var best = -1000.0;
    for (var k = 0; k < n; k = k + 1) {
        let a = (f32(k) + 0.5) / f32(n);
        let u = u_center + (a - 0.5) * 2.0 * U.half_span_u;
        let db = textureSampleLevel(db_tex, db_samp, vec2<f32>(clamp(u, 0.0, 1.0), v), 0.0).r;
        best = max(best, db);
    }

    let floor_db = U.ref_db - U.range_db;
    let t = clamp((best - floor_db) / U.range_db, 0.0, 1.0);
    return vec4<f32>(ramp(pow(t, 1.15)), 1.0);
}
"#;

/// GPU resources, created once and stashed in egui's render state.
pub struct WaterfallRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    uniform_buf: wgpu::Buffer,
    sampler: wgpu::Sampler,
    texture: Option<wgpu::Texture>,
    view: Option<wgpu::TextureView>,
    row_width: usize,
    /// Set when a newly allocated ring still needs filling with the noise floor.
    pending_clear: bool,
}

impl WaterfallRenderer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hfsdr waterfall shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hfsdr waterfall bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<WaterfallUniforms>() as u64,
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        // R32Float is not filterable everywhere, so sample nearest
                        // and do the resampling explicitly in the shader.
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hfsdr waterfall layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hfsdr waterfall pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(target_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hfsdr waterfall uniforms"),
            size: std::mem::size_of::<WaterfallUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("hfsdr waterfall sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            bind_group_layout,
            bind_group: None,
            uniform_buf,
            sampler,
            texture: None,
            view: None,
            row_width: 0,
            pending_clear: false,
        }
    }


    /// (Re)allocate the ring texture when the row width changes.
    fn ensure_texture(&mut self, device: &wgpu::Device, row_width: usize) {
        if self.texture.is_some() && self.row_width == row_width {
            return;
        }
        let width = row_width.clamp(1, device.limits().max_texture_dimension_2d as usize);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hfsdr waterfall dB ring"),
            size: wgpu::Extent3d {
                width: width as u32,
                height: RING_ROWS as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // A fresh R32Float texture reads as 0.0, and 0 dB is full scale — an
        // uncleared ring paints solid white until 360 rows have arrived.
        self.pending_clear = true;
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hfsdr waterfall bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }));
        self.texture = Some(texture);
        self.view = Some(view);
        self.row_width = width;
    }

    /// Fill the whole ring with the noise floor so unwritten rows read dark.
    fn clear_to_floor(&mut self, queue: &wgpu::Queue) {
        if !self.pending_clear || self.row_width == 0 {
            return;
        }
        let floor_row = vec![DB_FLOOR; self.row_width];
        for row in 0..RING_ROWS {
            self.write_row(queue, row, &floor_row);
        }
        self.pending_clear = false;
    }

    /// Upload one dB row into ring slot `row`.
    fn write_row(&self, queue: &wgpu::Queue, row: usize, db: &[f32]) {
        let (Some(texture), true) = (&self.texture, self.row_width > 0) else {
            return;
        };
        let mut padded;
        let data = if db.len() == self.row_width {
            db
        } else {
            padded = vec![DB_FLOOR; self.row_width];
            let n = db.len().min(self.row_width);
            padded[..n].copy_from_slice(&db[..n]);
            &padded
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: (row % RING_ROWS) as u32,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck_lite::bytes_of_slice(data),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some((self.row_width * 4) as u32),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: self.row_width as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// One frame's worth of work handed to the render pass.
pub struct WaterfallCallback {
    pub uniforms: WaterfallUniforms,
    /// Rows appended since the last paint, oldest first, with their ring slots.
    pub new_rows: Vec<(usize, Vec<f32>)>,
    pub row_width: usize,
}

impl egui_wgpu::CallbackTrait for WaterfallCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = resources.get_mut::<WaterfallRenderer>() {
            r.ensure_texture(device, self.row_width);
            r.clear_to_floor(queue);
            for (slot, row) in &self.new_rows {
                r.write_row(queue, *slot, row);
            }
            queue.write_buffer(&r.uniform_buf, 0, bytemuck_lite::bytes_of(&self.uniforms));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(r) = resources.get::<WaterfallRenderer>() else {
            return;
        };
        let Some(bind_group) = &r.bind_group else {
            return;
        };
        pass.set_pipeline(&r.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}


/// Register the renderer in egui's callback resources.
///
/// Returns false when the app is not running on the wgpu backend, in which case
/// the caller keeps the CPU waterfall path.
pub fn install(cc: &eframe::CreationContext<'_>) -> bool {
    let Some(state) = cc.wgpu_render_state.as_ref() else {
        return false;
    };
    let renderer = WaterfallRenderer::new(&state.device, state.target_format);
    state
        .renderer
        .write()
        .callback_resources
        .insert(renderer);
    true
}

/// Paint the GPU waterfall into `rect`.
pub fn paint(painter: &eframe::egui::Painter, rect: Rect, cb: WaterfallCallback) {
    painter.add(egui_wgpu::Callback::new_paint_callback(rect, cb));
}


/// Headless render of the shader into an RGBA target, for verification.
///
/// Returns `None` when no wgpu adapter exists (headless CI without a software
/// rasterizer), so the caller can skip rather than fail.
#[cfg(test)]
pub fn render_offscreen(
    rows: &[Vec<f32>],
    row_head: usize,
    out_w: u32,
    out_h: u32,
    u: WaterfallUniforms,
) -> Option<Vec<[u8; 4]>> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut r = WaterfallRenderer::new(&device, format);
    let row_width = rows.first().map(|r| r.len()).unwrap_or(1);
    r.ensure_texture(&device, row_width);
    for (i, row) in rows.iter().enumerate() {
        r.write_row(&queue, i, row);
    }
    queue.write_buffer(&r.uniform_buf, 0, bytemuck_lite::bytes_of(&u));
    let _ = row_head;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen target"),
        size: wgpu::Extent3d { width: out_w, height: out_h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let tview = target.create_view(&wgpu::TextureViewDescriptor::default());

    // Readback rows must be 256-byte aligned.
    let unpadded = out_w as usize * 4;
    let padded = unpadded.div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * out_h as usize) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("offscreen pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &tview,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&r.pipeline);
        pass.set_bind_group(0, r.bind_group.as_ref()?, &[]);
        pass.draw(0..3, 0..1);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(out_h),
            },
        },
        wgpu::Extent3d { width: out_w, height: out_h, depth_or_array_layers: 1 },
    );
    queue.submit(Some(enc.finish()));

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((out_w * out_h) as usize);
    for y in 0..out_h as usize {
        let base = y * padded;
        for x in 0..out_w as usize {
            let p = base + x * 4;
            out.push([data[p], data[p + 1], data[p + 2], data[p + 3]]);
        }
    }
    drop(data);
    readback.unmap();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The WGSL must actually compile and run. A syntax or binding error here
    /// only surfaces at device time, never at `cargo check`.
    #[test]
    fn shader_compiles_and_renders() {
        let rows: Vec<Vec<f32>> = (0..RING_ROWS)
            .map(|_| vec![-20.0f32; 64])
            .collect();
        let u = uniforms_for(0.0, 1.0, 0, 64, 32.0, -20.0, 80.0);
        let Some(px) = render_offscreen(&rows, 0, 32, 8, u) else {
            eprintln!("skipping: no wgpu adapter");
            return;
        };
        assert_eq!(px.len(), 32 * 8);
        // Full-scale dB with ref_db = -20 => top of the ramp => near white.
        assert!(px[0][0] > 200 && px[0][1] > 200, "expected bright, got {:?}", px[0]);
    }

    /// The shader ramp must agree with `dsp::colormap` — two implementations of
    /// one ramp is exactly the drift the CPU-side LUT bug came from.
    #[test]
    fn shader_ramp_matches_cpu_ramp() {
        let levels = [-100.0f32, -80.0, -60.0, -40.0, -20.0];
        let width = levels.len();
        let rows: Vec<Vec<f32>> = (0..RING_ROWS).map(|_| levels.to_vec()).collect();
        // One output pixel per source bin: no peak-hold, direct comparison.
        let u = uniforms_for(0.0, 1.0, 0, width, width as f32, -20.0, 80.0);
        let Some(px) = render_offscreen(&rows, 0, width as u32, 4, u) else {
            eprintln!("skipping: no wgpu adapter");
            return;
        };
        for (i, &db) in levels.iter().enumerate() {
            let cpu = hfsdr::db_to_rgba(db, -20.0, 80.0);
            let gpu = px[i];
            for c in 0..3 {
                let d = (cpu[c] as i32 - gpu[c] as i32).abs();
                assert!(
                    d <= 6,
                    "ramp mismatch at {db} dB channel {c}: cpu {cpu:?} gpu {gpu:?}"
                );
            }
        }
    }

    /// Peak-hold is the reason this is a shader and not a UV trick: a one-bin
    /// carrier must survive zoom-out instead of being averaged into the floor.
    #[test]
    fn peak_hold_preserves_a_single_bin_carrier() {
        let width = 512;
        let mut row = vec![-100.0f32; width];
        row[256] = -20.0; // lone carrier
        let rows: Vec<Vec<f32>> = (0..RING_ROWS).map(|_| row.clone()).collect();

        // 512 bins squeezed into 16 pixels: 32 bins per pixel.
        let u = uniforms_for(0.0, 1.0, 0, width, 16.0, -20.0, 80.0);
        assert!(u.taps > 1.0, "this test needs peak-hold engaged");
        let Some(px) = render_offscreen(&rows, 0, 16, 4, u) else {
            eprintln!("skipping: no wgpu adapter");
            return;
        };
        let brightest = px[..16].iter().map(|p| p[0] as u32).max().unwrap_or(0);
        assert!(
            brightest > 180,
            "carrier was lost in downsampling; brightest red was {brightest}"
        );
    }

    #[test]
    fn uniforms_cover_the_requested_window() {
        let u = uniforms_for(0.25, 0.75, 0, 4096, 1000.0, -20.0, 80.0);
        assert!((u.u0 - 0.25).abs() < 1e-6);
        assert!((u.u1 - 0.75).abs() < 1e-6);
        assert_eq!(u.row_count, RING_ROWS as f32);
    }

    /// Zoomed out, one pixel covers many bins and peak-hold must widen; zoomed in
    /// it must collapse to a single tap so the shader does no needless work.
    #[test]
    fn taps_track_zoom_level() {
        // 4096 bins across 1000 px, full span: ~4.1 bins per pixel.
        let out = uniforms_for(0.0, 1.0, 0, 4096, 1000.0, -20.0, 80.0);
        assert!(out.taps >= 4.0 && out.taps <= 5.0, "got {}", out.taps);

        // Zoomed to 1/10th: ~0.41 bins per pixel -> a single tap.
        let zoomed = uniforms_for(0.45, 0.55, 0, 4096, 1000.0, -20.0, 80.0);
        assert_eq!(zoomed.taps, 1.0);
    }

    #[test]
    fn taps_are_capped() {
        let extreme = uniforms_for(0.0, 1.0, 0, 65_536, 100.0, -20.0, 80.0);
        assert_eq!(extreme.taps, MAX_TAPS as f32);
    }

    #[test]
    fn row_head_wraps_into_the_ring() {
        let u = uniforms_for(0.0, 1.0, RING_ROWS + 7, 1024, 800.0, -20.0, 80.0);
        assert_eq!(u.row_head, 7.0);
    }

    #[test]
    fn zero_range_is_not_a_divide_by_zero() {
        let u = uniforms_for(0.0, 1.0, 0, 1024, 800.0, -20.0, 0.0);
        assert!(u.range_db.abs() > 0.0);
    }

    #[test]
    fn degenerate_window_does_not_produce_nan() {
        let u = uniforms_for(0.5, 0.5, 0, 1024, 800.0, -20.0, 80.0);
        assert!(u.u0.is_finite() && u.u1.is_finite());
        assert!(u.half_span_u.is_finite());
        assert!(u.taps >= 1.0);
    }

    #[test]
    fn uniform_block_is_tightly_packed() {
        // The shader declares 8 consecutive f32s; padding here would misalign them.
        assert_eq!(std::mem::size_of::<WaterfallUniforms>(), 32);
        assert_eq!(std::mem::size_of::<WaterfallUniforms>() % 16, 0);
    }
}
