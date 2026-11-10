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

/// Quads then text, drawn in that order. `backdrop` = re-blur the scene first.
struct Batch<'a> {
    backdrop: bool,
    quads: Range<u32>,
    texts: Vec<&'a TextCmd>,
    /// The one picture this batch's image quads sample (index into `DrawList::images`).
    image: Option<usize>,
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
    copy: wgpu::TextureView,
    /// Blur pyramid: mip i is the scene downsampled 2^(i+1) times.
    mips: Vec<Level>,
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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
        let copy = self.texture(size.0, size.1, 1).create_view(&Default::default());
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
        let quad_bind = Self::quad_bind(&self.device, &self.quad_layout, &self.globals, &copy, &all, &self.sampler);
        self.targets = Some(Targets { size, scene, copy, mips, quad_bind });
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

        // ---- 1. split the draw list into batches
        let mut quads = std::mem::take(&mut self.scratch);
        quads.clear();
        let new_batch = |backdrop: bool, at: u32| Batch { backdrop, quads: at..at, texts: Vec::new(), image: None };
        let mut batches: Vec<Batch> = vec![new_batch(false, 0)];
        // Glass samples a snapshot of the scene. A new snapshot (and blur pyramid) is only
        // needed when something was drawn since the last one *under this pane*; `dirty`
        // holds what was drawn since then. A row of glass buttons shares one snapshot.
        let mut dirty: Vec<Rect> = Vec::new();
        let mut snapshot = 0u32; // 0 = none taken yet
        let mut panes: Vec<(usize, u32, Rect)> = Vec::new(); // glass quad index, snapshot, rect
        for cmd in &list.cmds {
            let n = quads.len() as u32;
            let cur = batches.last_mut().unwrap();
            match cmd {
                Cmd::Quad(q) => {
                    let r = Rect::new(q.rect[0], q.rect[1], q.rect[2], q.rect[3]);
                    let glass = q.p0[3] == KIND_GLASS;
                    let grown = r.inflate(if glass { 1.0 } else { q.p0[2] * 3.0 + 1.0 });
                    let stale = glass && (snapshot == 0 || dirty.iter().any(|d| d.intersects(&grown)));
                    // A quad drawn after text that it overlaps must not end up under it.
                    let over_text =
                        cur.texts.iter().any(|t| Rect::new(t.pos.0, t.pos.1, t.size.0, t.size.1).intersects(&grown));
                    // One picture per draw call: a different one starts a new batch.
                    let picture = (q.p0[3] == KIND_IMAGE).then_some(q.c1[0] as usize);
                    let id = |i: usize| list.images.get(i).map(|m| m.0.id);
                    let other_image = matches!((picture, cur.image), (Some(a), Some(b)) if id(a) != id(b));
                    if stale {
                        batches.push(new_batch(true, n));
                        dirty.clear();
                        snapshot += 1;
                    } else if over_text || other_image {
                        batches.push(new_batch(false, n));
                    }
                    let cur = batches.last_mut().unwrap();
                    if picture.is_some() {
                        cur.image = picture;
                    }
                    if glass {
                        panes.push((quads.len(), snapshot, r));
                    }
                    // A pane marks only its own area: its neighbours may share the snapshot.
                    dirty.push(if glass { r } else { grown });
                    quads.push(*q);
                    cur.quads.end = n + 1;
                }
                Cmd::Text(t) => {
                    dirty.push(Rect::new(t.pos.0, t.pos.1, t.size.0, t.size.1));
                    cur.texts.push(t);
                }
            }
        }
        let has_glass = snapshot > 0;

        // Liquid: panes that sit very close to each other (and look at the same snapshot)
        // are fused into one shape by the shader. Each pane learns its two nearest neighbours.
        for a in 0..panes.len() {
            let (qa, snap, ra) = panes[a];
            let mut near: Vec<(f32, usize)> = panes
                .iter()
                .enumerate()
                .filter(|(b, p)| *b != a && p.1 == snap)
                .map(|(_, p)| {
                    let dx = (ra.x - (p.2.x + p.2.w)).max(p.2.x - (ra.x + ra.w)).max(0.0);
                    let dy = (ra.y - (p.2.y + p.2.h)).max(p.2.y - (ra.y + ra.h)).max(0.0);
                    ((dx * dx + dy * dy).sqrt(), p.0)
                })
                .filter(|(gap, _)| *gap < FUSE_GAP)
                .collect();
            near.sort_by(|x, y| x.0.total_cmp(&y.0));
            for (slot, (_, qb)) in near.into_iter().take(2).enumerate() {
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
        let mut used_text = 0;
        let mut text_slot: Vec<Option<usize>> = Vec::with_capacity(batches.len());
        for b in &batches {
            if b.texts.is_empty() {
                text_slot.push(None);
                continue;
            }
            if used_text == self.text_renderers.len() {
                self.text_renderers.push(TextRenderer::new(&mut self.atlas, &self.device, Default::default(), None));
            }
            let entries = &text.entries;
            let areas = b.texts.iter().flat_map(|t| {
                let e = entries.get(&t.key);
                let c = t.color;
                let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                let px = |r: &Rect| TextBounds {
                    left: (r.x * scale).floor() as i32,
                    top: (r.y * scale).floor() as i32,
                    right: ((r.x + r.w) * scale).ceil() as i32,
                    bottom: ((r.y + r.h) * scale).ceil() as i32,
                };
                // Glyphon has no per-pixel alpha, so a fade is the same text drawn again in
                // thin vertical strips, each clipped to its strip and a little fainter.
                let mut parts: Vec<(TextBounds, f32)> = Vec::new();
                match t.fade {
                    None => parts.push((px(&t.clip), 1.0)),
                    Some(f) => {
                        // Pixel edges shared by neighbouring parts, so no row or column is drawn twice.
                        let clip = px(&t.clip);
                        let top = ((f.y * scale).round() as i32).clamp(clip.top, clip.bottom);
                        let left = ((f.x * scale).round() as i32).clamp(clip.left, clip.right);
                        let right = (((f.x + f.w) * scale).round() as i32).clamp(left, clip.right);
                        let line = TextBounds { top, bottom: ((f.y + f.h) * scale).ceil().min(clip.bottom as f32) as i32, ..clip };
                        parts.push((TextBounds { bottom: top, ..clip }, 1.0));
                        parts.push((TextBounds { right: left, ..line }, 1.0));
                        let n = ((right - left) / 2).clamp(1, 24);
                        for k in 0..n {
                            let (l, r) = (left + (right - left) * k / n, left + (right - left) * (k + 1) / n);
                            let a = 1.0 - (k as f32 + 0.5) / n as f32;
                            parts.push((TextBounds { left: l, right: r, ..line }, a * a * (3.0 - 2.0 * a)));
                        }
                    }
                }
                parts.into_iter().filter(|(b, _)| b.right > b.left && b.bottom > b.top).filter_map(move |(bounds, a)| {
                    Some(TextArea {
                        buffer: &e?.buffer,
                        // The painter put snapped text on whole pixels; float noise (20.999998)
                        // must not fall a pixel short where glyph rows are truncated.
                        left: on_pixel(t.pos.0 * scale),
                        top: on_pixel(t.pos.1 * scale),
                        // Animated scale is snapped to 1/16 steps: otherwise every frame of a
                        // press or menu animation rasterizes each glyph at a brand-new size and
                        // floods the glyph atlas.
                        scale: (t.scale * 16.0).round() / 16.0 * scale,
                        bounds,
                        default_color: glyphon::Color::rgba(to8(c.r), to8(c.g), to8(c.b), to8(c.a * a)),
                        custom_glyphs: &[],
                    })
                })
            });
            let ok = self.text_renderers[used_text]
                .prepare(&self.device, &self.queue, &mut text.fonts, &mut self.atlas, &self.viewport, areas, &mut self.swash)
                .is_ok();
            text_slot.push(ok.then_some(used_text));
            used_text += 1;
        }

        // ---- 3. draw
        if has_glass {
            self.ensure_targets(size, scale);
        }
        let levels = self.targets.as_ref().map_or(0, |t| t.mips.len()) as f32;
        let g = Globals { screen: [size.0 as f32, size.1 as f32], scale, levels };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));
        let targets = if has_glass { self.targets.as_ref() } else { None };
        let (scene_view, bind) = match targets {
            Some(t) => (&t.scene.view, &t.quad_bind),
            None => (target, &self.quad_bind_plain),
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        // Everything is blended premultiplied, so the clear is too: a translucent background
        // over a transparent window then tints the desktop instead of brightening it.
        let a = clear.a.clamp(0.0, 1.0) as f64;
        let clear = wgpu::Color { r: clear.r as f64 * a, g: clear.g as f64 * a, b: clear.b as f64 * a, a };
        let mut load = wgpu::LoadOp::Clear(clear);
        let mut i = 0;
        while i < batches.len() {
            if let (true, Some(t)) = (batches[i].backdrop, targets) {
                // Snapshot everything drawn so far (sharp), then build its blur pyramid.
                Self::fullscreen(&mut encoder, &self.blit, &t.scene, &t.copy);
                Self::fullscreen(&mut encoder, &self.down, &t.scene, &t.mips[0].view);
                for l in 1..t.mips.len() {
                    Self::fullscreen(&mut encoder, &self.down, &t.mips[l - 1], &t.mips[l].view);
                }
            }
            let mut j = i + 1;
            while j < batches.len() && !batches[j].backdrop {
                j += 1;
            }
            {
                let mut pass = Self::pass(&mut encoder, scene_view, load);
                for (k, b) in batches[i..j].iter().enumerate() {
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
                    if let Some(slot) = text_slot[i + k] {
                        let _ = self.text_renderers[slot].render(&self.atlas, &self.viewport, &mut pass);
                    }
                }
            }
            load = wgpu::LoadOp::Load;
            i = j;
        }
        if let Some(t) = targets {
            Self::fullscreen(&mut encoder, &self.blit, &t.scene, target);
        }
        self.queue.submit(Some(encoder.finish()));
        self.atlas.trim();
        drop(batches);
        self.scratch = quads;
    }
}

/// `v` rounded when it is a whole number give or take float error, else as is.
fn on_pixel(v: f32) -> f32 {
    let r = v.round();
    if (v - r).abs() < 1e-3 { r } else { v }
}
