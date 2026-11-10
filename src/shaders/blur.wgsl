// Backdrop pyramid for glass: each level is a 2x downsample of the one before. Glass
// samples it at a continuous level, so one pyramid serves every blur radius on screen.

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

// 2x downsample with the 13-tap filter from Jimenez, "Next Generation Post Processing in
// Call of Duty: Advanced Warfare": wide enough that a moving backdrop doesn't shimmer.
@fragment
fn down(in: VOut) -> @location(0) vec4f {
    let t = 1.0 / vec2f(textureDimensions(src));
    let a = textureSample(src, samp, in.uv + t * vec2f(-2.0, -2.0));
    let b = textureSample(src, samp, in.uv + t * vec2f(0.0, -2.0));
    let c = textureSample(src, samp, in.uv + t * vec2f(2.0, -2.0));
    let d = textureSample(src, samp, in.uv + t * vec2f(-1.0, -1.0));
    let e = textureSample(src, samp, in.uv + t * vec2f(1.0, -1.0));
    let f = textureSample(src, samp, in.uv + t * vec2f(-2.0, 0.0));
    let g = textureSample(src, samp, in.uv);
    let h = textureSample(src, samp, in.uv + t * vec2f(2.0, 0.0));
    let i = textureSample(src, samp, in.uv + t * vec2f(-1.0, 1.0));
    let j = textureSample(src, samp, in.uv + t * vec2f(1.0, 1.0));
    let k = textureSample(src, samp, in.uv + t * vec2f(-2.0, 2.0));
    let l = textureSample(src, samp, in.uv + t * vec2f(0.0, 2.0));
    let m = textureSample(src, samp, in.uv + t * vec2f(2.0, 2.0));
    return (d + e + i + j) * 0.125 + g * 0.125 + (b + f + h + l) * 0.0625 + (a + c + k + m) * 0.03125;
}

@fragment
fn blit(in: VOut) -> @location(0) vec4f {
    return textureSample(src, samp, in.uv);
}
