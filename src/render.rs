//! wgpu renderer: turns a `DrawList` into as few draw calls as possible.
//!
//! Everything is one instanced pipeline. Glass needs the already-drawn scene
//! blurred, so the frame is split only where a glass layer starts; without any
//! glass the frame goes straight to the screen in a single pass.

use crate::text::TextSystem;
use crate::types::{Color, Rect};
use crate::ui::{Cmd, DrawList, Quad, TextCmd, KIND_GLASS, KIND_IMAGE};
use crate::view::Image;
use std::collections::HashMap;
use glyphon::{Cache, ColorMode, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport};
use std::ops::Range;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    screen: [f32; 2],
    scale: f32,
    /// Number of mips in the backdrop pyramid.
    levels: f32,
}

/// Quads then text, drawn in that order.
struct Batch {
    /// Snapshot to take first (index into `Renderer::snaps`): glass here samples the scene so far.
    snap: Option<usize>,
    quads: Range<u32>,
    /// Range of `Renderer::text_cmds`, which index `DrawList::cmds`.
    texts: Range<u32>,
    /// The one picture this batch's image quads sample (index into `DrawList::images`).
    image: Option<usize>,
}

/// Deepest pyramid we ever build (the shader blends two neighbouring levels).
const MAX_MIPS: usize = 8;

/// Area in pixels, edges as floats; empty when inverted.
#[derive(Clone, Copy, PartialEq)]
struct PxBox {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl PxBox {
    const EMPTY: PxBox = PxBox { x0: f32::MAX, y0: f32::MAX, x1: f32::MIN, y1: f32::MIN };
    fn of(r: [f32; 4], s: f32) -> Self {
        PxBox { x0: r[0] * s, y0: r[1] * s, x1: (r[0] + r[2]) * s, y1: (r[1] + r[3]) * s }
    }
    fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    fn union(&self, o: &PxBox) -> PxBox {
        PxBox { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    fn intersect(&self, o: &PxBox) -> PxBox {
        PxBox { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    fn grow(&self, d: f32) -> PxBox {
        if self.is_empty() {
            return *self;
        }
        PxBox { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }
    /// The texels of a `dims` texture spread over `screen` pixels that touch this box.
    fn texels(&self, screen: (u32, u32), dims: (u32, u32)) -> IBox {
        let (kx, ky) = (dims.0 as f32 / screen.0 as f32, dims.1 as f32 / screen.1 as f32);
        if self.is_empty() {
            return IBox::default();
        }
        let c = |v: f32, max: u32| (v.max(0.0) as u32).min(max);
        IBox {
            x0: c((self.x0 * kx).floor(), dims.0),
            y0: c((self.y0 * ky).floor(), dims.1),
            x1: c((self.x1 * kx).ceil(), dims.0),
            y1: c((self.y1 * ky).ceil(), dims.1),
        }
    }
}

/// Texel area of one texture, `x1`/`y1` exclusive.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct IBox {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl IBox {
    fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    fn meets(&self, o: &IBox) -> bool {
        !self.is_empty() && !o.is_empty() && self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
}

/// What a snapshot writes: the part of the sharp copy and of each pyramid level that the
/// glass reading it can reach (directly, or through the levels above).
#[derive(Clone, Copy, PartialEq, Default, Debug)]
struct Plan {
    copy: IBox,
    /// Screen pixels the snapshot is made from: the copy, and what level 0 filters.
    source: IBox,
    mips: u32,
    levels: [IBox; MAX_MIPS],
}

impl Plan {
    fn meets(&self, o: &Plan) -> bool {
        self.copy.meets(&o.copy) || (0..self.mips.min(o.mips) as usize).any(|k| self.levels[k].meets(&o.levels[k]))
    }
    fn words(&self, out: &mut Vec<u32>) {
        out.push(self.mips);
        for b in std::iter::once(&self.copy).chain(&self.levels[..self.mips as usize]) {
            out.extend_from_slice(&[b.x0, b.y0, b.x1, b.y1]);
        }
    }
}

/// A snapshot the glass in one batch samples.
struct Snap {
    /// The glass command that needs it: everything drawn before is in the snapshot.
    cmd: usize,
    /// Pixels the panes read from it, and the deepest blur level they read.
    reach: PxBox,
    level: f32,
    plan: Plan,
    /// Same content as a snapshot still in the textures: nothing to build.
    reuse: bool,
}

/// A snapshot whose pixels are still in the textures, and what it was made of.
struct Cached {
    key: Vec<u32>,
    plan: Plan,
}

/// What the renderer did, for benchmarks and the FPS overlay. Counts only grow.
#[derive(Clone, Copy, Default, Debug)]
pub struct RenderStats {
    /// `render` calls.
    pub frames: u64,
    /// Frames `unchanged` found identical to the last one drawn.
    pub skipped: u64,
    /// Glass snapshots built (sharp copy + blur pyramid), and ones reused from earlier frames.
    pub pyramids: u64,
    pub pyramid_reuses: u64,
    /// Text batches handed to glyphon, and ones whose prepared glyphs were reused.
    pub prepares: u64,
    pub prepare_skips: u64,
}

/// Panes closer than this (points) fuse; `FUSE_K` is the reach of the liquid neck.
const FUSE_GAP: f32 = 13.0;
const FUSE_K: f32 = 26.0;

struct Level {
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

struct Targets {
    size: (u32, u32),
    scene: Level,
    /// Sharp copy of the scene: glass refracts it while the scene itself is drawn into.
    copy: Level,
    /// Blur pyramid: mip i is the scene downsampled 2^(i+1) times.
    mips: Vec<Level>,
    /// Size of pyramid level 0.
    pyramid: (u32, u32),
    quad_bind: wgpu::BindGroup,
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    globals: wgpu::Buffer,
    sampler: wgpu::Sampler,
    quad_pipeline: wgpu::RenderPipeline,
    /// Same shader, no blending: glass writes its finished pixel (see `quad.wgsl`).
    glass_pipeline: wgpu::RenderPipeline,
    quad_layout: wgpu::BindGroupLayout,
    quad_bind_plain: wgpu::BindGroup,
    instances: wgpu::Buffer,
    instance_cap: usize,
    blur_layout: wgpu::BindGroupLayout,
    down: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    targets: Option<Targets>,
    swash: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderers: Vec<TextRenderer>,
    scratch: Vec<Quad>,
    image_layout: wgpu::BindGroupLayout,
    image_none: wgpu::BindGroup,
    /// Uploaded pictures by id, with the frame they were last drawn in.
    images: HashMap<u64, (wgpu::BindGroup, u64)>,
    frame: u64,
    pub stats: RenderStats,
    /// The last frame drawn, flattened, and the one being looked at (see `unchanged`).
    drawn: Vec<u32>,
    now: Vec<u32>,
    drawn_valid: bool,
    // Per-frame lists, kept to reuse their memory.
    batches: Vec<Batch>,
    text_cmds: Vec<u32>,
    dirty: Vec<Rect>,
    panes: Vec<(usize, usize, Rect)>,
    snaps: Vec<Snap>,
    key: Vec<u32>,
    /// Snapshots whose pixels are in the copy and pyramid textures (their areas don't overlap).
    cached: Vec<Cached>,
    /// What each text renderer was last prepared with; frames since the atlas was trimmed.
    text_sigs: Vec<Vec<u32>>,
    since_trim: u32,
    /// Glass was drawn into a target that could not be copied from.
    wanted_copy: bool,
}

fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ruru globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ruru linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // ---- quad pipeline
        let quad_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ruru quad"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1),
                tex_entry(2),
                sampler_entry(3),
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ruru image"),
            entries: &[tex_entry(0)],
        });
        let quad_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/quad.wgsl"));
        let quad_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ruru quad"),
            bind_group_layouts: &[Some(&quad_layout), Some(&image_layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x4, 9 => Float32x4
        ];
        let target = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let strip = wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() };
        let glass_target = [Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })];
        let quad_pipeline_with = |label: &str, target: &[Option<wgpu::ColorTargetState>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&quad_pl),
                vertex: wgpu::VertexState {
                    module: &quad_shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Quad>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &attrs,
                    })],
                },
                primitive: strip,
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &quad_shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let quad_pipeline = quad_pipeline_with("ruru quad", &target);
        let glass_pipeline = quad_pipeline_with("ruru glass", &glass_target);

        // ---- blur pipelines
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ruru blur"),
            entries: &[tex_entry(0), sampler_entry(1)],
        });
        let blur_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/blur.wgsl"));
        let blur_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ruru blur"),
            bind_group_layouts: &[Some(&blur_layout)],
            immediate_size: 0,
        });
        let opaque = [Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })];
        let fullscreen = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&blur_pl),
                vertex: wgpu::VertexState {
                    module: &blur_shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &blur_shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &opaque,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let (down, blit) = (fullscreen("down"), fullscreen("blit"));

        // Bound when a frame has no glass at all.
        let dummy = device.create_texture_with_data(
            &queue,
            &wgpu::TextureDescriptor {
                label: Some("ruru dummy"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            Default::default(),
            &[0, 0, 0, 255],
        );
        let dummy_view = dummy.create_view(&Default::default());
        let quad_bind_plain = Self::quad_bind(&device, &quad_layout, &globals, &dummy_view, &dummy_view, &sampler);

        let image_none = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ruru no image"),
            layout: &image_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&dummy_view) }],
        });

        let instance_cap = 256;
        let instances = Self::instance_buffer(&device, instance_cap);

        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mode = if format.is_srgb() { ColorMode::Accurate } else { ColorMode::Web };
        let atlas = TextAtlas::with_color_mode(&device, &queue, &cache, format, mode);

        Renderer {
            device,
            queue,
            format,
            globals,
            sampler,
            quad_pipeline,
            glass_pipeline,
            quad_layout,
            quad_bind_plain,
            instances,
            instance_cap,
            blur_layout,
            down,
            blit,
            targets: None,
            swash: SwashCache::new(),
            viewport,
            atlas,
            text_renderers: Vec::new(),
            scratch: Vec::new(),
            image_layout,
            image_none,
            images: HashMap::new(),
            frame: 0,
            stats: RenderStats::default(),
            drawn: Vec::new(),
            now: Vec::new(),
            drawn_valid: false,
            batches: Vec::new(),
            text_cmds: Vec::new(),
            dirty: Vec::new(),
            panes: Vec::new(),
            snaps: Vec::new(),
            key: Vec::new(),
            cached: Vec::new(),
            text_sigs: Vec::new(),
            since_trim: 0,
            wanted_copy: false,
        }
    }

    /// Uploads a picture with a full mip chain (box filter) the first time it is drawn.
    fn upload_image(&mut self, img: &Image) {
        let frame = self.frame;
        if let Some(e) = self.images.get_mut(&img.0.id) {
            e.1 = frame;
            return;
        }
        let (w, h) = (img.0.width.max(1), img.0.height.max(1));
        let mips = 32 - w.max(h).leading_zeros();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ruru image"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (mut mw, mut mh) = (w, h);
        let mut level: Vec<u8> = img.0.pixels.clone();
        for mip in 0..mips {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &level,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(mw * 4), rows_per_image: None },
                wgpu::Extent3d { width: mw, height: mh, depth_or_array_layers: 1 },
            );
            let (nw, nh) = ((mw / 2).max(1), (mh / 2).max(1));
            let mut next = vec![0u8; (nw * nh * 4) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    let (x0, y0) = ((x * 2).min(mw - 1), (y * 2).min(mh - 1));
                    let (x1, y1) = ((x0 + 1).min(mw - 1), (y0 + 1).min(mh - 1));
                    for c in 0..4 {
                        let at = |px: u32, py: u32| level[((py * mw + px) * 4 + c) as usize] as u32;
                        next[((y * nw + x) * 4 + c) as usize] =
                            ((at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1) + 2) / 4) as u8;
                    }
                }
            }
            level = next;
            (mw, mh) = (nw, nh);
        }
        let view = texture.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ruru image"),
            layout: &self.image_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
        });
        self.images.insert(img.0.id, (bind, frame));
    }

    fn instance_buffer(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ruru instances"),
            size: (cap * std::mem::size_of::<Quad>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn quad_bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        globals: &wgpu::Buffer,
        sharp: &wgpu::TextureView,
        pyramid: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ruru quad"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(sharp) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(pyramid) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    fn texture(&self, w: u32, h: u32, mips: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ruru layer"),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// A view plus a bind group that samples exactly that view.
    fn level(&self, view: wgpu::TextureView) -> Level {
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ruru layer"),
            layout: &self.blur_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        Level { view, bind }
    }

    fn ensure_targets(&mut self, size: (u32, u32), scale: f32) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let scene = self.level(self.texture(size.0, size.1, 1).create_view(&Default::default()));
        let copy = self.level(self.texture(size.0, size.1, 1).create_view(&Default::default()));
        // One pyramid serves every blur radius: mip i blurs by roughly 2^(i+1) px.
        // An extra level on high-DPI screens keeps the maximum blur the same in points.
        let (pw, ph) = (size.0.div_ceil(2), size.1.div_ceil(2));
        let fit = 32 - pw.min(ph).max(1).leading_zeros();
        let n = (if scale >= 1.5 { 6 } else { 5 }).min(fit);
        let pyramid = self.texture(pw, ph, n);
        let mips: Vec<Level> = (0..n)
            .map(|i| {
                self.level(pyramid.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: i,
                    mip_level_count: Some(1),
                    ..Default::default()
                }))
            })
            .collect();
        let all = pyramid.create_view(&Default::default());
        let quad_bind = Self::quad_bind(&self.device, &self.quad_layout, &self.globals, &copy.view, &all, &self.sampler);
        self.targets = Some(Targets { size, pyramid: (pw, ph), scene, copy, mips, quad_bind });
        self.cached.clear();
    }

    fn pass<'e>(
        encoder: &'e mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
    ) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    fn fullscreen(encoder: &mut wgpu::CommandEncoder, pipe: &wgpu::RenderPipeline, src: &Level, dst: &wgpu::TextureView) {
        let mut p = Self::pass(encoder, dst, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
        p.set_pipeline(pipe);
        p.set_bind_group(0, &src.bind, &[]);
        p.draw(0..3, 0..1);
    }

    /// Runs `pipe` over `dst`, only inside `area` (texels of `dst`). Loads what is there:
    /// other snapshots kept in the same texture must survive.
    fn filter(encoder: &mut wgpu::CommandEncoder, pipe: &wgpu::RenderPipeline, src: &Level, dst: &wgpu::TextureView, area: IBox) {
        if area.is_empty() {
            return;
        }
        let mut p = Self::pass(encoder, dst, wgpu::LoadOp::Load);
        p.set_scissor_rect(area.x0, area.y0, area.x1 - area.x0, area.y1 - area.y0);
        p.set_pipeline(pipe);
        p.set_bind_group(0, &src.bind, &[]);
        p.draw(0..3, 0..1);
    }

    /// True if this frame would draw exactly what the last `render` drew, so the caller can
    /// skip acquiring and presenting a frame: the screen already shows it. Hover without a
    /// visible change, or a wake-up that changed nothing, then costs no GPU work at all.
    pub fn unchanged(&mut self, size: (u32, u32), scale: f32, list: &DrawList, clear: Color) -> bool {
        frame_words(&mut self.now, size, scale, list, clear);
        let same = self.drawn_valid && self.now == self.drawn;
        if same {
            self.stats.skipped += 1;
        }
        same
    }

    /// True once glass was drawn into a target without `COPY_SRC` usage. With it, glass
    /// frames are drawn straight into the target, saving an off-screen pass over the whole
    /// screen (about 0.2 ms at 2164×1360 on an M-series GPU).
    pub fn wants_copy_src(&self) -> bool {
        self.wanted_copy
    }

    /// Forget the last frame: the next `unchanged` is false. Call when the surface was
    /// reconfigured or lost, as its pixels can no longer be trusted.
    pub fn invalidate(&mut self) {
        self.drawn_valid = false;
    }

    /// Renders `list` into `target`. `size` is in physical pixels, `scale` is points→pixels.
    pub fn render(
        &mut self,
        target: &wgpu::TextureView,
        size: (u32, u32),
        scale: f32,
        list: &DrawList,
        text: &mut TextSystem,
        clear: Color,
    ) {
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        self.stats.frames += 1;
        // Remembered for `unchanged` once drawn (flattened here too: a caller may not ask).
        frame_words(&mut self.now, size, scale, list, clear);

        // ---- 1. split the draw list into batches
        let mut quads = std::mem::take(&mut self.scratch);
        let mut batches = std::mem::take(&mut self.batches);
        let mut text_cmds = std::mem::take(&mut self.text_cmds);
        let mut dirty = std::mem::take(&mut self.dirty);
        let mut panes = std::mem::take(&mut self.panes);
        let mut snaps = std::mem::take(&mut self.snaps);
        quads.clear();
        batches.clear();
        text_cmds.clear();
        dirty.clear();
        panes.clear();
        snaps.clear();
        let new_batch = |snap: Option<usize>, at: u32, t: u32| Batch { snap, quads: at..at, texts: t..t, image: None };
        batches.push(new_batch(None, 0, 0));
        // Bounds of the current batch's texts: most quads miss all of them, and then the
        // texts need not be looked at one by one.
        let mut text_box: Option<Rect> = None;
        // Glass samples a snapshot of the scene. A new snapshot (and blur pyramid) is only
        // needed when something was drawn since the last one *under this pane*; `dirty`
        // holds what was drawn since then. A row of glass buttons shares one snapshot.
        for (ci, cmd) in list.cmds.iter().enumerate() {
            let n = quads.len() as u32;
            let t = text_cmds.len() as u32;
            let cur = batches.last_mut().unwrap();
            match cmd {
                Cmd::Quad(q) => {
                    let r = Rect::new(q.rect[0], q.rect[1], q.rect[2], q.rect[3]);
                    let glass = q.p0[3] == KIND_GLASS;
                    let grown = r.inflate(if glass { 1.0 } else { q.p0[2] * 3.0 + 1.0 });
                    let stale = glass && (snaps.is_empty() || dirty.iter().any(|d| d.intersects(&grown)));
                    // A quad drawn after text that it overlaps must not end up under it.
                    let over_text = text_box.is_some_and(|b| b.intersects(&grown))
                        && text_cmds[cur.texts.start as usize..].iter().any(|&i| match &list.cmds[i as usize] {
                            Cmd::Text(t) => Rect::new(t.pos.0, t.pos.1, t.size.0, t.size.1).intersects(&grown),
                            Cmd::Quad(_) => false,
                        });
                    // One picture per draw call: a different one starts a new batch.
                    let picture = (q.p0[3] == KIND_IMAGE).then_some(q.c1[0] as usize);
                    let id = |i: usize| list.images.get(i).map(|m| m.0.id);
                    let other_image = matches!((picture, cur.image), (Some(a), Some(b)) if id(a) != id(b));
                    if stale {
                        batches.push(new_batch(Some(snaps.len()), n, t));
                        snaps.push(Snap { cmd: ci, reach: PxBox::EMPTY, level: f32::MIN, plan: Plan::default(), reuse: false });
                        dirty.clear();
                        text_box = None;
                    } else if over_text || other_image {
                        batches.push(new_batch(None, n, t));
                        text_box = None;
                    }
                    let cur = batches.last_mut().unwrap();
                    if picture.is_some() {
                        cur.image = picture;
                    }
                    if glass {
                        panes.push((quads.len(), snaps.len() - 1, r));
                    }
                    // A pane marks only its own area: its neighbours may share the snapshot.
                    dirty.push(if glass { r } else { grown });
                    quads.push(*q);
                    cur.quads.end = n + 1;
                }
                Cmd::Text(tc) => {
                    let r = Rect::new(tc.pos.0, tc.pos.1, tc.size.0, tc.size.1);
                    dirty.push(r);
                    text_box = Some(text_box.map_or(r, |b| union(&b, &r)));
                    text_cmds.push(ci as u32);
                    cur.texts.end = t + 1;
                }
            }
        }
        let has_glass = !snaps.is_empty();

        // Liquid: panes that sit very close to each other (and look at the same snapshot)
        // are fused into one shape by the shader. Each pane learns its two nearest neighbours.
        for a in 0..panes.len() {
            let (qa, snap, ra) = panes[a];
            let mut near = [(f32::MAX, 0usize); 2];
            for (b, p) in panes.iter().enumerate() {
                if b == a || p.1 != snap {
                    continue;
                }
                let dx = (ra.x - (p.2.x + p.2.w)).max(p.2.x - (ra.x + ra.w)).max(0.0);
                let dy = (ra.y - (p.2.y + p.2.h)).max(p.2.y - (ra.y + ra.h)).max(0.0);
                let gap = (dx * dx + dy * dy).sqrt();
                // Ties keep the earlier pane, as a stable sort would.
                if gap < FUSE_GAP && gap < near[1].0 {
                    near[1] = (gap, p.0);
                    if near[1].0 < near[0].0 {
                        near.swap(0, 1);
                    }
                }
            }
            for (slot, (gap, qb)) in near.into_iter().enumerate() {
                if gap == f32::MAX {
                    break;
                }
                let (rect, radius) = (quads[qb].rect, quads[qb].p0[0]);
                let q = &mut quads[qa];
                if slot == 0 {
                    q.n0 = rect;
                } else {
                    q.n1 = rect;
                }
                q.np[slot] = radius;
                q.np[2] = FUSE_K;
            }
        }

        // ---- 2. upload
        self.frame += 1;
        for b in &batches {
            if let Some(img) = b.image.and_then(|i| list.images.get(i)) {
                self.upload_image(img);
            }
        }
        if self.images.len() > 64 {
            let frame = self.frame;
            self.images.retain(|_, e| e.1 + 600 > frame);
        }

        if quads.len() > self.instance_cap {
            self.instance_cap = quads.len().next_power_of_two();
            self.instances = Self::instance_buffer(&self.device, self.instance_cap);
        }
        self.queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&quads));

        self.viewport.update(&self.queue, Resolution { width: size.0, height: size.1 });
        self.prepare_text(&batches, &text_cmds, list, text, size, scale);

        // ---- 3. plan the snapshots
        // Rendering straight into the target saves a full-screen copy at the end; it needs
        // the target readable, to copy what is under the glass out of it.
        let tex = target.texture();
        let direct = has_glass
            && tex.usage().contains(wgpu::TextureUsages::COPY_SRC)
            && tex.format() == self.format
            && (tex.width(), tex.height()) == size
            && tex.mip_level_count() == 1;
        self.wanted_copy |= has_glass && !direct;
        if has_glass {
            self.ensure_targets(size, scale);
            for &(qi, s, _) in &panes {
                let (reach, level) = glass_reach(&quads[qi], scale);
                let sn = &mut snaps[s];
                sn.reach = sn.reach.union(&reach);
                sn.level = sn.level.max(level);
            }
            let t = self.targets.as_ref().unwrap();
            let levels = t.mips.len() as u32;
            for sn in snaps.iter_mut() {
                sn.plan = plan(sn.reach, sn.level, levels, size, t.pyramid, direct);
                // What the snapshot is made of: everything drawn before it that reaches its area.
                let key = &mut self.key;
                key.clear();
                key.extend_from_slice(&[direct as u32, size.0, size.1, scale.to_bits()]);
                key.extend_from_slice(&[clear.r.to_bits(), clear.g.to_bits(), clear.b.to_bits(), clear.a.to_bits()]);
                sn.plan.words(key);
                snap_key(key, list, &quads, sn.cmd, sn.plan.source, scale);
                sn.reuse = self.cached.iter().any(|c| c.key == *key && c.plan == sn.plan);
                if sn.reuse {
                    self.stats.pyramid_reuses += 1;
                } else {
                    self.stats.pyramids += 1;
                    // Its pixels overwrite whatever else was kept there.
                    let p = sn.plan;
                    self.cached.retain(|c| !c.plan.meets(&p));
                    if self.cached.len() >= 8 {
                        self.cached.remove(0);
                    }
                    self.cached.push(Cached { key: key.clone(), plan: p });
                }
            }
        }

        // ---- 4. draw
        let levels = self.targets.as_ref().map_or(0, |t| t.mips.len()) as f32;
        let g = Globals { screen: [size.0 as f32, size.1 as f32], scale, levels };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        let targets = if has_glass { self.targets.as_ref() } else { None };
        let (scene_view, bind) = match targets {
            Some(t) if !direct => (&t.scene.view, &t.quad_bind),
            Some(t) => (target, &t.quad_bind),
            None => (target, &self.quad_bind_plain),
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        // Everything is blended premultiplied, so the clear is too: a translucent background
        // over a transparent window then tints the desktop instead of brightening it.
        let a = clear.a.clamp(0.0, 1.0) as f64;
        let clear = wgpu::Color { r: clear.r as f64 * a, g: clear.g as f64 * a, b: clear.b as f64 * a, a };
        let mut load = wgpu::LoadOp::Clear(clear);
        let mut i = 0;
        let mut text_slot = 0;
        while i < batches.len() {
            if let (Some(s), Some(t)) = (batches[i].snap, targets) {
                let sn = &snaps[s];
                if !sn.reuse {
                    // Snapshot what is drawn so far (sharp), then build its blur pyramid,
                    // each only where the glass can reach.
                    let c = sn.plan.copy;
                    if direct {
                        let origin = wgpu::Origin3d { x: c.x0, y: c.y0, z: 0 };
                        let extent = wgpu::Extent3d { width: c.x1 - c.x0, height: c.y1 - c.y0, depth_or_array_layers: 1 };
                        if !c.is_empty() {
                            encoder.copy_texture_to_texture(
                                wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin, aspect: wgpu::TextureAspect::All },
                                wgpu::TexelCopyTextureInfo {
                                    texture: t.copy.view.texture(),
                                    mip_level: 0,
                                    origin,
                                    aspect: wgpu::TextureAspect::All,
                                },
                                extent,
                            );
                        }
                    } else {
                        Self::filter(&mut encoder, &self.blit, &t.scene, &t.copy.view, c);
                    }
                    let first = if direct { &t.copy } else { &t.scene };
                    for l in 0..sn.plan.mips as usize {
                        let src = if l == 0 { first } else { &t.mips[l - 1] };
                        Self::filter(&mut encoder, &self.down, src, &t.mips[l].view, sn.plan.levels[l]);
                    }
                }
            }
            let mut j = i + 1;
            while j < batches.len() && batches[j].snap.is_none() {
                j += 1;
            }
            {
                let mut pass = Self::pass(&mut encoder, scene_view, load);
                for b in &batches[i..j] {
                    if !b.quads.is_empty() {
                        pass.set_bind_group(0, bind, &[]);
                        let picture = b.image.and_then(|i| list.images.get(i)).and_then(|m| self.images.get(&m.0.id));
                        pass.set_bind_group(1, picture.map_or(&self.image_none, |e| &e.0), &[]);
                        pass.set_vertex_buffer(0, self.instances.slice(..));
                        // Runs of glass and of everything else, each with its own blending.
                        let is_glass = |i: u32| quads[i as usize].p0[3] == KIND_GLASS;
                        let mut a = b.quads.start;
                        while a < b.quads.end {
                            let glass = is_glass(a);
                            let mut e = a + 1;
                            while e < b.quads.end && is_glass(e) == glass {
                                e += 1;
                            }
                            pass.set_pipeline(if glass { &self.glass_pipeline } else { &self.quad_pipeline });
                            pass.draw(0..4, a..e);
                            a = e;
                        }
                    }
                    if !b.texts.is_empty() {
                        if self.text_sigs[text_slot].is_empty() {
                            // glyphon failed to prepare it (atlas full): nothing to draw.
                        } else {
                            let _ = self.text_renderers[text_slot].render(&self.atlas, &self.viewport, &mut pass);
                        }
                        text_slot += 1;
                    }
                }
            }
            load = wgpu::LoadOp::Load;
            i = j;
        }
        if let (Some(t), false) = (targets, direct) {
            Self::fullscreen(&mut encoder, &self.blit, &t.scene, target);
        }
        self.queue.submit(Some(encoder.finish()));

        std::mem::swap(&mut self.now, &mut self.drawn);
        self.drawn_valid = true;

        self.scratch = quads;
        self.batches = batches;
        self.text_cmds = text_cmds;
        self.dirty = dirty;
        self.panes = panes;
        self.snaps = snaps;
    }

    /// Hands each text batch to its glyphon renderer. A batch with exactly the same texts as
    /// that renderer had last frame keeps its prepared glyphs. That is only safe while those
    /// glyphs stay in the atlas: glyphon evicts only glyphs not marked in use, and the marks
    /// are cleared (`trim`) only on a frame that prepares every batch afresh.
    fn prepare_text(
        &mut self,
        batches: &[Batch],
        text_cmds: &[u32],
        list: &DrawList,
        text: &mut TextSystem,
        size: (u32, u32),
        scale: f32,
    ) {
        let text_of = |i: u32| match &list.cmds[i as usize] {
            Cmd::Text(t) => Some(t),
            Cmd::Quad(_) => None,
        };
        let sig = |b: &Batch, out: &mut Vec<u32>| {
            out.clear();
            out.extend_from_slice(&[size.0, size.1, scale.to_bits()]);
            for t in text_cmds[b.texts.start as usize..b.texts.end as usize].iter().filter_map(|&i| text_of(i)) {
                text_words(out, t);
            }
        };
        let mut slots = 0;
        let mut same = 0;
        let mut buf = std::mem::take(&mut self.key);
        for b in batches.iter().filter(|b| !b.texts.is_empty()) {
            sig(b, &mut buf);
            if self.text_sigs.get(slots).is_some_and(|s| *s == buf) {
                same += 1;
            }
            slots += 1;
        }
        // Keep the in-use marks from growing for ever while some batch never changes.
        let fresh = same == 0 || self.since_trim >= 30;
        if fresh {
            self.atlas.trim();
            self.since_trim = 0;
        } else {
            self.since_trim += 1;
        }
        let mut slot = 0;
        for b in batches.iter().filter(|b| !b.texts.is_empty()) {
            if slot == self.text_renderers.len() {
                self.text_renderers.push(TextRenderer::new(&mut self.atlas, &self.device, Default::default(), None));
                self.text_sigs.push(Vec::new());
            }
            sig(b, &mut buf);
            if !fresh && self.text_sigs[slot] == buf {
                self.stats.prepare_skips += 1;
                slot += 1;
                continue;
            }
            let (fonts, buffers) = text.for_drawing();
            let areas = text_cmds[b.texts.start as usize..b.texts.end as usize].iter().filter_map(|&i| text_of(i)).flat_map(|t| {
                let e = buffers.get(t.key);
                let c = t.color;
                let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                fade_parts(t, scale).filter(|(b, _)| b.right > b.left && b.bottom > b.top).filter_map(move |(bounds, a)| {
                    Some(TextArea {
                        buffer: e?,
                        // The painter put snapped text on whole pixels; float noise (20.999998)
                        // must not fall a pixel short where glyph rows are truncated.
                        left: on_pixel(t.pos.0 * scale),
                        // Glyphon truncates the row; the painter already put it on a whole
                        // pixel, this only keeps an unsnapped one from falling short.
                        top: (t.pos.1 * scale).round(),
                        // Already quantized by the painter (`text_scale`), which places the
                        // text for exactly this scale.
                        scale: t.scale * scale,
                        bounds,
                        default_color: glyphon::Color::rgba(to8(c.r), to8(c.g), to8(c.b), to8(c.a * a)),
                        custom_glyphs: &[],
                    })
                })
            });
            self.stats.prepares += 1;
            let ok = self.text_renderers[slot]
                .prepare(&self.device, &self.queue, fonts, &mut self.atlas, &self.viewport, areas, &mut self.swash)
                .is_ok();
            // An empty signature never matches: a failed batch is retried and not drawn.
            std::mem::swap(&mut self.text_sigs[slot], &mut buf);
            if !ok {
                self.text_sigs[slot].clear();
            }
            slot += 1;
        }
        self.key = buf;
    }
}

/// Glyphon has no per-pixel alpha, so a fade is the same text drawn again in thin vertical
/// strips, each clipped to its strip and a little fainter. Without a fade: the clip alone.
fn fade_parts(t: &TextCmd, scale: f32) -> impl Iterator<Item = (TextBounds, f32)> {
    let px = |r: &Rect| TextBounds {
        left: (r.x * scale).floor() as i32,
        top: (r.y * scale).floor() as i32,
        right: ((r.x + r.w) * scale).ceil() as i32,
        bottom: ((r.y + r.h) * scale).ceil() as i32,
    };
    let clip = px(&t.clip);
    let none = TextBounds { left: 0, top: 0, right: 0, bottom: 0 };
    let (head, start, line, left, right, n) = match t.fade {
        None => (clip, none, none, 0, 0, 0),
        Some(f) => {
            // Pixel edges shared by neighbouring parts, so no row or column is drawn twice.
            let top = ((f.y * scale).round() as i32).clamp(clip.top, clip.bottom);
            let left = ((f.x * scale).round() as i32).clamp(clip.left, clip.right);
            let right = (((f.x + f.w) * scale).round() as i32).clamp(left, clip.right);
            let line = TextBounds { top, bottom: ((f.y + f.h) * scale).ceil().min(clip.bottom as f32) as i32, ..clip };
            (TextBounds { bottom: top, ..clip }, TextBounds { right: left, ..line }, line, left, right, ((right - left) / 2).clamp(1, 24))
        }
    };
    [(head, 1.0), (start, 1.0)].into_iter().chain((0..n).map(move |k| {
        let (l, r) = (left + (right - left) * k / n, left + (right - left) * (k + 1) / n);
        let a = 1.0 - (k as f32 + 0.5) / n as f32;
        (TextBounds { left: l, right: r, ..line }, a * a * (3.0 - 2.0 * a))
    }))
}

fn union(a: &Rect, b: &Rect) -> Rect {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    Rect::new(x0, y0, (a.x + a.w).max(b.x + b.w) - x0, (a.y + a.h).max(b.y + b.h) - y0)
}

/// Pixels a glass pane may read from its snapshot, and the deepest blur level it reads
/// (mirrors `vs` and `fs` in `quad.wgsl`).
fn glass_reach(q: &Quad, s: f32) -> (PxBox, f32) {
    // Drawn: the quad grown as the vertex shader grows it, inside its clip.
    let grow = 1.0 + q.p0[2] * s * 3.0 + q.np[2] * s * 0.5;
    let drawn = PxBox::of(q.rect, s).grow(grow).intersect(&PxBox::of(q.clip, s).grow(1.0));
    if drawn.is_empty() {
        return (PxBox::EMPTY, f32::MIN);
    }
    // Refraction looks sideways by at most depth * refraction * tan of the largest bend,
    // which for a ray through a surface at any tilt is sqrt(eta^2 - 1).
    let half = q.rect[2].min(q.rect[3]) * s * 0.5;
    let depth = (q.c1[1] * s).max(1.0).min((half * 0.6).max(1.0));
    let eta = 1.5 + (q.c1[3] * 0.25).max(0.0);
    let bend = q.p1[1].max(0.0) * depth * (eta * eta - 1.0).sqrt();
    let sigma = q.p1[0] * 0.5 * s;
    // Plus two pixels for bilinear taps.
    (drawn.grow(bend + 2.0), (sigma.max(1e-4) / 1.9).log2() + 1.0)
}

/// Size of pyramid level `k` when level 0 is `base`.
fn mip_dims(base: (u32, u32), k: u32) -> (u32, u32) {
    ((base.0 >> k).max(1), (base.1 >> k).max(1))
}

/// Which texels of the copy and of each pyramid level a snapshot must fill so that glass
/// reading `reach` up to blur `level` sees exactly what a full-screen build would give.
fn plan(reach: PxBox, level: f32, levels: u32, screen: (u32, u32), base: (u32, u32), direct: bool) -> Plan {
    let full = PxBox { x0: 0.0, y0: 0.0, x1: screen.0 as f32, y1: screen.1 as f32 };
    // Same arithmetic as `frost`: level <= 0 reads only the sharp copy; otherwise the two
    // levels around it. A hair of slack in case the GPU rounds the log differently.
    let mips = if levels == 0 || level <= -0.01 { 0 } else { ((level + 0.01).floor().max(0.0) as u32).min(levels - 1) + 1 };
    let texel = |k: u32| {
        let d = mip_dims(base, k);
        (screen.0 as f32 / d.0 as f32).max(screen.1 as f32 / d.1 as f32)
    };
    let mut p = Plan { copy: IBox::default(), source: IBox::default(), mips, levels: [IBox::default(); MAX_MIPS] };
    // Glass reads ±2.5 texels (B-spline taps); a level reads ±2.5 texels of the one below
    // around each of its texels, which may stick out a texel past the area asked of it.
    let mut b = reach.grow(4.0 * texel(mips.saturating_sub(1)));
    for k in (0..mips).rev() {
        if k + 1 < mips {
            b = b.grow(2.0 * texel(k + 1) + 3.0 * texel(k));
        }
        p.levels[k as usize] = b.intersect(&full).texels(screen, mip_dims(base, k));
    }
    let copy = reach.grow(2.0);
    let source = if mips > 0 { copy.union(&b.grow(2.0 * texel(0) + 3.0)) } else { copy };
    // Level 0 is built from the copy when drawing straight into the target, else from the scene.
    p.copy = if direct { source } else { copy }.intersect(&full).texels(screen, screen);
    p.source = source.intersect(&full).texels(screen, screen);
    p
}

/// Area a text may paint: glyphs can stick out of the layout box (italics, accents),
/// never by much more than a line.
fn text_reach(t: &TextCmd) -> Rect {
    let k = t.scale.max(1.0);
    let pad = t.size.1.clamp(8.0, 64.0) * k;
    Rect::new(t.pos.0, t.pos.1, t.size.0 * k, t.size.1 * k).inflate(pad).intersect(&t.clip)
}

/// Appends what a snapshot taken before command `upto` holds inside `area` (texels of the
/// screen): every earlier command that paints there. Glass already there shows what was
/// under *it*, possibly from far away, so then everything before counts.
fn snap_key(out: &mut Vec<u32>, list: &DrawList, quads: &[Quad], upto: usize, area: IBox, scale: f32) {
    let r = Rect::new(area.x0 as f32 / scale, area.y0 as f32 / scale, (area.x1 - area.x0) as f32 / scale, (area.y1 - area.y0) as f32 / scale)
        .inflate(1.0);
    let quad_reach = |q: &Quad| Rect::new(q.rect[0], q.rect[1], q.rect[2], q.rect[3]).inflate(q.p0[2] * 3.0 + q.np[2] * 0.5 + 1.0);
    let cmds = &list.cmds[..upto];
    let mut qi = 0;
    let mut all = false;
    for c in cmds {
        if let Cmd::Quad(_) = c {
            let q = &quads[qi];
            qi += 1;
            if q.p0[3] == KIND_GLASS && quad_reach(q).intersects(&r) {
                all = true;
                break;
            }
        }
    }
    let mut qi = 0;
    for c in cmds {
        match c {
            Cmd::Quad(_) => {
                let q = &quads[qi];
                qi += 1;
                if all || quad_reach(q).intersects(&r) {
                    out.push(0);
                    out.extend_from_slice(bytemuck::cast_slice(std::slice::from_ref(q)));
                    if q.p0[3] == KIND_IMAGE {
                        let id = list.images.get(q.c1[0] as usize).map_or(0, |m| m.0.id);
                        out.extend_from_slice(&[id as u32, (id >> 32) as u32]);
                    }
                }
            }
            Cmd::Text(t) => {
                if all || text_reach(t).intersects(&r) {
                    out.push(1);
                    text_words(out, t);
                }
            }
        }
    }
}

fn text_words(out: &mut Vec<u32>, t: &TextCmd) {
    let f = |v: f32| v.to_bits();
    let (c, k) = (t.color, t.clip);
    out.extend_from_slice(&[t.key as u32, (t.key >> 32) as u32, f(t.pos.0), f(t.pos.1), f(t.size.0), f(t.size.1)]);
    out.extend_from_slice(&[f(t.scale), f(c.r), f(c.g), f(c.b), f(c.a), f(k.x), f(k.y), f(k.w), f(k.h)]);
    match t.fade {
        Some(r) => out.extend_from_slice(&[1, f(r.x), f(r.y), f(r.w), f(r.h)]),
        None => out.push(0),
    }
}

/// The whole frame as plain numbers: equal numbers, equal pixels.
fn frame_words(out: &mut Vec<u32>, size: (u32, u32), scale: f32, list: &DrawList, clear: Color) {
    out.clear();
    out.extend_from_slice(&[size.0, size.1, scale.to_bits()]);
    out.extend_from_slice(&[clear.r.to_bits(), clear.g.to_bits(), clear.b.to_bits(), clear.a.to_bits()]);
    for c in &list.cmds {
        match c {
            Cmd::Quad(q) => {
                out.push(0);
                out.extend_from_slice(bytemuck::cast_slice(std::slice::from_ref(q)));
            }
            Cmd::Text(t) => {
                out.push(1);
                text_words(out, t);
            }
        }
    }
    for m in &list.images {
        out.extend_from_slice(&[m.0.id as u32, (m.0.id >> 32) as u32]);
    }
}

/// `v` rounded when it is a whole number give or take float error, else as is.
fn on_pixel(v: f32) -> f32 {
    let r = v.round();
    if (v - r).abs() < 1e-3 { r } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_covers_what_glass_reads_and_what_each_level_needs() {
        let reach = PxBox { x0: 400.0, y0: 900.0, x1: 1600.0, y1: 1100.0 };
        let (screen, base) = ((2000, 1200), (1000, 600));
        let p = plan(reach, 3.4, 6, screen, base, true);
        assert_eq!(p.mips, 4, "blur level 3.4 blends levels 2 and 3");
        let texels = |b: &PxBox, k: u32| b.texels(screen, mip_dims(base, k));
        let inside = |a: IBox, b: IBox| a.x0 >= b.x0 && a.y0 >= b.y0 && a.x1 <= b.x1 && a.y1 <= b.y1;
        for k in 0..p.mips {
            // What glass reads (±3 texels) is built...
            assert!(inside(texels(&reach.grow(3.0 * (2 << k) as f32), k), p.levels[k as usize]));
        }
        for k in 1..p.mips as usize {
            // ...and every level is built from a larger area of the one below.
            let up = p.levels[k];
            let below = IBox { x0: up.x0 * 2, y0: up.y0 * 2, x1: (up.x1 * 2).min(1000 >> (k - 1)), y1: (up.y1 * 2).min(600 >> (k - 1)) };
            assert!(inside(below, p.levels[k - 1]), "level {k}");
        }
        assert!(inside(reach.grow(2.0).texels(screen, screen), p.copy), "the sharp copy covers the reach");
        let level0 = p.levels[0];
        let feeds = IBox { x0: level0.x0 * 2 - 3, y0: level0.y0 * 2 - 3, x1: (level0.x1 * 2 + 3).min(2000), y1: (level0.y1 * 2 + 3).min(1200) };
        assert!(inside(feeds, p.source), "the key covers what level 0 is filtered from");
        assert_eq!(p.copy, p.source, "drawing into the target, level 0 is filtered from the copy");
        // Clamped to the screen; nothing outside.
        assert!(p.levels[0].x1 <= 1000 && p.levels[0].y1 <= 600 && p.copy.y1 <= 1200);
        // Without blur only the sharp copy is needed.
        assert_eq!(plan(reach, -2.0, 6, screen, base, true).mips, 0);
    }

    #[test]
    fn frame_words_tell_frames_apart() {
        let mut list = DrawList::default();
        let q: Quad = bytemuck::Zeroable::zeroed();
        list.cmds.push(Cmd::Quad(q));
        let (mut a, mut b) = (Vec::new(), Vec::new());
        frame_words(&mut a, (100, 100), 2.0, &list, Color::BLACK);
        frame_words(&mut b, (100, 100), 2.0, &list, Color::BLACK);
        assert_eq!(a, b);
        if let Cmd::Quad(q) = &mut list.cmds[0] {
            q.c0[3] = 0.5;
        }
        frame_words(&mut b, (100, 100), 2.0, &list, Color::BLACK);
        assert_ne!(a, b);
        frame_words(&mut a, (100, 100), 1.0, &list, Color::BLACK);
        assert_ne!(a, b, "scale counts");
    }
}
