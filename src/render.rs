//! wgpu renderer: turns a `DrawList` into as few draw calls as possible.
//!
//! Everything is one instanced pipeline. Glass needs the already-drawn scene
//! blurred, so the frame is split only where a glass layer starts; without any
//! glass the frame goes straight to the screen in a single pass.

use crate::text::TextSystem;
use crate::types::{Color, Rect};
use crate::ui::{Cmd, DrawList, Quad, TextCmd, KIND_GLASS};
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
    only_glass: bool,
    glass_rects: Vec<Rect>,
    texts: Vec<&'a TextCmd>,
}

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
        let quad_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/quad.wgsl"));
        let quad_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ruru quad"),
            bind_group_layouts: &[Some(&quad_layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4
        ];
        let target = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let strip = wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() };
        let quad_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ruru quad"),
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
                targets: &target,
            }),
            multiview_mask: None,
            cache: None,
        });

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
        }
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
        let new_batch = |backdrop: bool, at: u32| Batch {
            backdrop,
            quads: at..at,
            only_glass: backdrop,
            glass_rects: Vec::new(),
            texts: Vec::new(),
        };
        let mut batches: Vec<Batch> = vec![new_batch(false, 0)];
        for cmd in &list.cmds {
            let n = quads.len() as u32;
            let cur = batches.last_mut().unwrap();
            match cmd {
                Cmd::Quad(q) => {
                    let r = Rect::new(q.rect[0], q.rect[1], q.rect[2], q.rect[3]);
                    if q.p0[3] == KIND_GLASS {
                        // Glass panes that don't overlap can share one blur of the scene.
                        let share = cur.backdrop && cur.only_glass && !cur.glass_rects.iter().any(|g| g.intersects(&r));
                        if !share {
                            batches.push(new_batch(true, n));
                        }
                        batches.last_mut().unwrap().glass_rects.push(r);
                    } else {
                        // A quad drawn after text that it overlaps must not end up under it.
                        let grown = r.inflate(q.p0[2] * 3.0 + 1.0);
                        let over_text =
                            cur.texts.iter().any(|t| Rect::new(t.pos.0, t.pos.1, t.size.0, t.size.1).intersects(&grown));
                        if over_text {
                            batches.push(new_batch(false, n));
                        }
                        batches.last_mut().unwrap().only_glass = false;
                    }
                    quads.push(*q);
                    batches.last_mut().unwrap().quads.end = n + 1;
                }
                Cmd::Text(t) => {
                    cur.only_glass = false;
                    cur.texts.push(t);
                }
            }
        }
        let has_glass = batches.iter().any(|b| b.backdrop);

        // ---- 2. upload
        
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
            let areas = b.texts.iter().filter_map(|t| {
                let e = entries.get(&t.key)?;
                let c = t.color;
                let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                Some(TextArea {
                    buffer: &e.buffer,
                    left: t.pos.0 * scale,
                    top: t.pos.1 * scale,
                    scale: t.scale * scale,
                    bounds: TextBounds {
                        left: (t.clip.x * scale).floor() as i32,
                        top: (t.clip.y * scale).floor() as i32,
                        right: ((t.clip.x + t.clip.w) * scale).ceil() as i32,
                        bottom: ((t.clip.y + t.clip.h) * scale).ceil() as i32,
                    },
                    default_color: glyphon::Color::rgba(to8(c.r), to8(c.g), to8(c.b), to8(c.a)),
                    custom_glyphs: &[],
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
        let clear = wgpu::Color { r: clear.r as f64, g: clear.g as f64, b: clear.b as f64, a: 1.0 };
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
                        pass.set_pipeline(&self.quad_pipeline);
                        pass.set_bind_group(0, bind, &[]);
                        pass.set_vertex_buffer(0, self.instances.slice(..));
                        pass.draw(0..4, b.quads.clone());
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
