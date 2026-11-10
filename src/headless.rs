//! Off-screen rendering to PNG: screenshots, visual tests, CI.

use crate::render::Renderer;
use crate::types::Color;
use crate::ui::Ui;
use std::path::Path;

/// Renders the UI at `width`×`height` points and `scale` pixels per point into a PNG file.
pub fn render_png<S: 'static>(ui: &mut Ui<S>, width: u32, height: u32, scale: f32, path: impl AsRef<Path>) {
    render_png_after(ui, width, height, scale, 1, |_, _| {}, path)
}

/// Like [`render_png`], but renders `frames` frames with one renderer and saves the last.
/// `before` runs ahead of every frame (index from 0) to change state or send input.
pub fn render_png_after<S: 'static>(
    ui: &mut Ui<S>,
    width: u32,
    height: u32,
    scale: f32,
    frames: u32,
    mut before: impl FnMut(&mut Ui<S>, u32),
    path: impl AsRef<Path>,
) {
    let (pw, ph) = ((width as f32 * scale) as u32, (height as f32 * scale) as u32);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("no GPU adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("request device");
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = Renderer::new(device.clone(), queue.clone(), format);

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ruru headless"),
        size: wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    for i in 0..frames.max(1) {
        before(ui, i);
        ui.frame((width as f32, height as f32));
        // A PNG has no desktop behind it: the background is drawn opaque, as before.
        renderer.render(&view, (pw, ph), scale, &ui.list, &mut ui.text, Color { a: 1.0, ..ui.background });
    }

    let row = (pw * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * ph) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        texture.size(),
    );
    queue.submit(Some(encoder.finish()));
    buffer.map_async(wgpu::MapMode::Read, .., |r| r.expect("map"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");

    let data = buffer.get_mapped_range(..).expect("mapped range");
    let mut pixels = Vec::with_capacity((pw * ph * 4) as usize);
    for y in 0..ph {
        let start = (y * row) as usize;
        pixels.extend_from_slice(&data[start..start + (pw * 4) as usize]);
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("create png"));
    let mut enc = png::Encoder::new(file, pw, ph);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("png header").write_image_data(&pixels).expect("png data");
}
