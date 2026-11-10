// Dual-Kawase blur: a few cheap down/up-sample passes give a wide, smooth blur.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.pos = vec4f(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2f(p.x, 1.0 - p.y);
    return o;
}

@fragment
fn down(in: VOut) -> @location(0) vec4f {
    let h = 1.0 / vec2f(textureDimensions(src));
    var c = textureSample(src, samp, in.uv) * 4.0;
    c += textureSample(src, samp, in.uv + vec2f(-h.x, -h.y));
    c += textureSample(src, samp, in.uv + vec2f(h.x, -h.y));
    c += textureSample(src, samp, in.uv + vec2f(-h.x, h.y));
    c += textureSample(src, samp, in.uv + vec2f(h.x, h.y));
    return c / 8.0;
}

@fragment
fn up(in: VOut) -> @location(0) vec4f {
    let h = 1.0 / vec2f(textureDimensions(src));
    var c = textureSample(src, samp, in.uv + vec2f(-2.0 * h.x, 0.0));
    c += textureSample(src, samp, in.uv + vec2f(2.0 * h.x, 0.0));
    c += textureSample(src, samp, in.uv + vec2f(0.0, -2.0 * h.y));
    c += textureSample(src, samp, in.uv + vec2f(0.0, 2.0 * h.y));
    c += textureSample(src, samp, in.uv + vec2f(-h.x, h.y)) * 2.0;
    c += textureSample(src, samp, in.uv + vec2f(h.x, h.y)) * 2.0;
    c += textureSample(src, samp, in.uv + vec2f(-h.x, -h.y)) * 2.0;
    c += textureSample(src, samp, in.uv + vec2f(h.x, -h.y)) * 2.0;
    return c / 12.0;
}

@fragment
fn blit(in: VOut) -> @location(0) vec4f {
    return textureSample(src, samp, in.uv);
}
