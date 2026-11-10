// One instanced quad draws every shape: fills, gradients, borders, shadows and glass.

struct Globals {
    screen: vec2f, // physical pixels
    scale: f32,    // points -> pixels
    levels: f32,   // mips in the backdrop pyramid
}
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var sharp: texture_2d<f32>; // the scene behind the glass, unblurred
@group(0) @binding(2) var pyr: texture_2d<f32>;   // its blur pyramid
@group(0) @binding(3) var samp: sampler;
@group(1) @binding(0) var img: texture_2d<f32>;   // picture for image quads

struct Instance {
    @location(0) rect: vec4f,
    @location(1) clip: vec4f,
    @location(2) c0: vec4f,
    @location(3) c1: vec4f,
    @location(4) border: vec4f,
    @location(5) p0: vec4f, // radius, border width, shadow sigma, kind
    @location(6) p1: vec4f, // gradient dir, clip radius, opacity
    @location(7) n0: vec4f, // glass: neighbour pane to fuse with (x, y, w, h)
    @location(8) n1: vec4f, // glass: second neighbour
    @location(9) np: vec4f, // neighbour radii, fusing distance k (0 = none)
}

struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) xy: vec2f, // pixel position
    @location(1) @interpolate(flat) rect: vec4f, // center.xy, half.xy (pixels)
    @location(2) @interpolate(flat) clip: vec4f, // center.xy, half.xy (pixels)
    @location(3) @interpolate(flat) c0: vec4f,
    @location(4) @interpolate(flat) c1: vec4f,
    @location(5) @interpolate(flat) border: vec4f,
    @location(6) @interpolate(flat) p0: vec4f,
    @location(7) @interpolate(flat) p1: vec4f,
    @location(8) @interpolate(flat) n0: vec4f, // center.xy, half.xy (pixels)
    @location(9) @interpolate(flat) n1: vec4f,
    @location(10) @interpolate(flat) np: vec4f,
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, in: Instance) -> VOut {
    let s = g.scale;
    let corner = vec2f(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let r = in.rect * s;
    // Room for AA, the shadow falloff, and the liquid neck between fused panes.
    let grow = 1.0 + in.p0.z * s * 3.0 + in.np.z * s * 0.5;
    let xy = r.xy - grow + corner * (r.zw + 2.0 * grow);

    var o: VOut;
    o.pos = vec4f(xy.x / g.screen.x * 2.0 - 1.0, 1.0 - xy.y / g.screen.y * 2.0, 0.0, 1.0);
    o.xy = xy;
    o.rect = vec4f(r.xy + r.zw * 0.5, r.zw * 0.5);
    let c = in.clip * s;
    o.clip = vec4f(c.xy + c.zw * 0.5, c.zw * 0.5);
    o.c0 = in.c0;
    o.c1 = in.c1;
    o.border = in.border;
    o.p0 = vec4f(in.p0.xyz * s, in.p0.w);
    o.p1 = vec4f(in.p1.xy, in.p1.z * s, in.p1.w);
    let a = in.n0 * s;
    let b = in.n1 * s;
    o.n0 = vec4f(a.xy + a.zw * 0.5, a.zw * 0.5);
    o.n1 = vec4f(b.xy + b.zw * 0.5, b.zw * 0.5);
    o.np = in.np * s;
    return o;
}

// Distance (x) and outward unit normal (yz) of a rounded box whose corners are
// superellipses ("squircles"), like iOS continuous corners; pills and discs fall back to
// true circles. The superellipse value is divided by its gradient length so it stays a
// real distance near the edge (crisp AA, correct bevel), and the normal is analytic.
// Inside the straight part the normal is blended across the diagonal over `soft` px so
// the glass bevel has no crease running into the corners.
// (Distance-with-gradient formulation after al1h3n/liquid-rust, MIT/Apache-2.0.)
fn sdg_shape(p: vec2f, half: vec2f, r_in: f32, soft: f32) -> vec3f {
    let m = min(half.x, half.y);
    let k = clamp(r_in / max(m, 0.001), 0.0, 1.0);
    let t = smoothstep(0.55, 1.0, k);
    let n = mix(2.8, 2.0, t);
    let r = min(r_in * mix(1.2, 1.0, t), m);
    let sg = select(vec2f(-1.0), vec2f(1.0), p >= vec2f(0.0));
    let w = abs(p) - half + r;
    let g = max(w.x, w.y);
    if g > 0.0 {
        let q = max(w, vec2f(1e-6));
        let l = pow(pow(q.x, n) + pow(q.y, n), 1.0 / n);
        let grad = pow(q / l, vec2f(n - 1.0));
        let gl = max(length(grad), 1e-6);
        return vec3f((l - r) / gl, sg * grad / gl);
    }
    let e = exp((w - vec2f(g)) / max(soft, 0.001));
    return vec3f(g - r, sg * normalize(e));
}

fn sd_shape(p: vec2f, half: vec2f, r_in: f32) -> f32 {
    return sdg_shape(p, half, r_in, 1.0).x;
}

// Quadratic smooth minimum (Inigo Quilez): the blended distance, and how much of `b` to
// mix into anything carried along with it (here: the normal).
fn smin_w(a: f32, b: f32, k: f32) -> vec2f {
    let h = max(k - abs(a - b), 0.0);
    let n = 0.5 * h / k;
    return vec2f(min(a, b) - 0.25 * h * h / k, select(1.0 - n, n, a < b));
}

// Smooth union with a neighbouring pane: where two panes nearly touch, their surfaces
// reach for each other and join with a liquid neck.
fn fuse(dg: vec3f, xy: vec2f, pane: vec4f, r: f32, k: f32, soft: f32) -> vec3f {
    if pane.z <= 0.0 {
        return dg;
    }
    let b = sdg_shape(xy - pane.xy, pane.zw, r, soft);
    let w = smin_w(dg.x, b.x, k);
    let n = mix(dg.yz, b.yz, w.y);
    return vec3f(w.x, n / max(length(n), 1e-4));
}

// Slope of a squircle-profile bevel h(t) = (1 - (1 - t)^4)^(1/4): flat in the middle of
// the pane, curving down steeply at the rim. t = 0 at the edge, 1 where the bevel ends.
fn bevel_slope(t: f32) -> f32 {
    let u = 1.0 - t;
    let u3 = u * u * u;
    return u3 * pow(max(1.0 - u3 * u, 1e-5), -0.75);
}

// How far sideways (in units of glass thickness) a ray looking straight down travels
// after refracting through a surface tilted by atan(slope). Snell's law.
fn refract_offset(slope: f32, eta: f32) -> f32 {
    let theta = atan(slope);
    return tan(theta - asin(clamp(sin(theta) / eta, -1.0, 1.0)));
}

// Bicubic B-spline fetch from one pyramid mip with 4 bilinear taps: hides the blocky
// look of a magnified low-resolution level.
fn bspline(uv: vec2f, mip: f32) -> vec4f {
    let size = vec2f(textureDimensions(pyr, u32(mip)));
    let t = uv * size - 0.5;
    let i = floor(t);
    let f = t - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - f) * (1.0 - f) * (1.0 - f) / 6.0;
    let w1 = (3.0 * f3 - 6.0 * f2 + 4.0) / 6.0;
    let w2 = (-3.0 * f3 + 3.0 * f2 + 3.0 * f + 1.0) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (i - 0.5 + w1 / g0) / size;
    let h1 = (i + 1.5 + w3 / g1) / size;
    let a = textureSampleLevel(pyr, samp, vec2f(h0.x, h0.y), mip);
    let b = textureSampleLevel(pyr, samp, vec2f(h1.x, h0.y), mip);
    let c = textureSampleLevel(pyr, samp, vec2f(h0.x, h1.y), mip);
    let d = textureSampleLevel(pyr, samp, vec2f(h1.x, h1.y), mip);
    return g0.y * (g0.x * a + g1.x * b) + g1.y * (g0.x * c + g1.x * d);
}

// The backdrop at pixel `q`, blurred by a gaussian of `sigma` px: the sharp scene for
// tiny radii, otherwise two neighbouring pyramid levels blended continuously.
// Level n of the 13-tap pyramid has a sigma of about 0.96 * 2^n px.
// Premultiplied: alpha < 1 where a transparent window shows the desktop.
fn frost(q: vec2f, sigma: f32) -> vec4f {
    let uv = q / g.screen;
    let level = log2(max(sigma, 1e-4) / 1.9) + 1.0;
    if level <= 0.0 || g.levels < 1.0 {
        return textureSampleLevel(sharp, samp, uv, 0.0);
    }
    if level < 1.0 {
        return mix(textureSampleLevel(sharp, samp, uv, 0.0), bspline(uv, 0.0), level);
    }
    let l = min(level, g.levels);
    let lo = min(floor(l), g.levels - 1.0);
    if lo < 1.0 {
        return bspline(uv, 0.0);
    }
    return mix(bspline(uv, lo - 1.0), bspline(uv, lo), clamp(l - lo, 0.0, 1.0));
}

fn sd_round_box(p: vec2f, half: vec2f, r: f32) -> f32 {
    let q = abs(p) - half + r;
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn erf(x: f32) -> f32 {
    let a = abs(x);
    let d = 1.0 + a * (0.278393 + a * (0.230389 + a * (0.000972 + a * 0.078108)));
    let d2 = d * d;
    return sign(x) * (1.0 - 1.0 / (d2 * d2));
}

fn premul(c: vec4f) -> vec4f {
    return vec4f(c.rgb * c.a, c.a);
}

fn noise(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs(in: VOut) -> @location(0) vec4f {
    let p = in.xy - in.rect.xy;
    let half = in.rect.zw;
    let radius = in.p0.x;
    let kind = in.p0.w;

    // Glass only: bevel width in px, never wider than the pane's half size.
    let depth = clamp(in.c1.y * g.scale, 1.0, max(min(half.x, half.y), 1.0));
    let soft = select(1.0, depth * 0.35, kind == 2.0);
    var dg = sdg_shape(p, half, radius, soft);
    if in.np.z > 0.0 {
        dg = fuse(dg, in.xy, in.n0, in.np.x, in.np.z, soft);
        dg = fuse(dg, in.xy, in.n1, in.np.y, in.np.z, soft);
    }
    let d = dg.x;
    let cover = clamp(0.5 - d, 0.0, 1.0);
    let clip = clamp(0.5 - sd_round_box(in.xy - in.clip.xy, in.clip.zw, in.p1.z), 0.0, 1.0);
    let alpha = clip * in.p1.w;

    if kind == 1.0 {
        // Soft shadow: gaussian falloff of the shape's distance field.
        let sigma = max(in.p0.z, 0.5);
        var a = 0.5 - 0.5 * erf(d / (sigma * 1.41421));
        if in.c1.z > 0.5 {
            // Don't darken underneath a translucent caster.
            a *= clamp(0.5 + sd_shape(p + in.c1.xy * g.scale, half, radius), 0.0, 1.0);
        }
        return premul(vec4f(in.c0.rgb, in.c0.a * a)) * alpha;
    }

    var col: vec4f;
    if kind == 2.0 {
        // Glass. The pane is flat in the middle and bevelled at the rim. The backdrop is only
        // lightly frosted and is refracted through that bevel (index 1.5), slightly
        // differently per colour channel: most of the look is bent light, not blur.
        let nrm = dg.yz;
        let t = clamp(-d / depth, 0.0, 1.0);
        let sigma = in.p1.x * 0.5 * g.scale;
        // The snapshot is premultiplied: the glass works on straight colour and keeps the
        // backdrop's alpha, so over a transparent window it stays as see-through as the
        // window around it instead of turning into dark, opaque premultiplied colour.
        var bg: vec3f;
        var ba: f32;
        if t < 1.0 && in.p1.y > 0.0 {
            let slope = bevel_slope(t);
            let reach = nrm * in.p1.y * depth;
            let disp = in.c1.w * 0.25;
            let mid = frost(in.xy - reach * refract_offset(slope, 1.5), sigma);
            ba = mid.a;
            bg = mid.rgb / max(mid.a, 1e-4);
            if disp > 0.0 {
                let r = frost(in.xy - reach * refract_offset(slope, 1.5 - disp), sigma);
                let b = frost(in.xy - reach * refract_offset(slope, 1.5 + disp), sigma);
                bg = vec3f(r.r / max(r.a, 1e-4), bg.g, b.b / max(b.a, 1e-4));
            }
        } else {
            let f = frost(in.xy, sigma);
            ba = f.a;
            bg = f.rgb / max(f.a, 1e-4);
        }
        let luma = dot(bg, vec3f(0.2126, 0.7152, 0.0722));
        bg = mix(vec3f(luma), bg, in.c1.x);
        // The tint is laid over the backdrop and adds its own coverage.
        let ga = ba + (1.0 - ba) * in.c0.a;
        bg = (bg * ba * (1.0 - in.c0.a) + in.c0.rgb * in.c0.a) / max(ga, 1e-4);

        // Specular rim: a crisp line that takes its colour from what is behind the glass,
        // strongest on the edge facing the light (top-left), weaker on the opposite one.
        let facing = dot(nrm, vec2f(-0.5145, -0.8575));
        let splay = 0.12;
        let lit = splay + (1.0 - splay) * pow(max(facing, 0.0), 2.0);
        let back = splay + (0.4 - splay) * pow(max(-facing, 0.0), 2.0);
        let line = exp(d / (0.55 * g.scale));
        let l2 = dot(bg, vec3f(0.3, 0.59, 0.11));
        let highlight = (vec3f(l2) + (bg - vec3f(l2)) * 2.2) * 1.45 + vec3f(0.05);
        bg = mix(bg, highlight, clamp(line * select(back, lit, facing > 0.0) * in.c1.z * 2.5, 0.0, 1.0));
        // Faint lift across the bevel gives the slab some thickness.
        bg += (1.0 - t) * (1.0 - t) * 0.05;
        bg += (noise(in.xy) - 0.5) / 255.0; // dither away banding
        col = vec4f(clamp(bg, vec3f(0.0), vec3f(1.0)) * ga, ga);
    } else if kind == 3.0 {
        // Picture: c0 is the visible part of it in UV space. Pick the mip that matches
        // the on-screen size so a large photo shown small doesn't shimmer.
        let uv = mix(in.c0.xy, in.c0.zw, p / (half * 2.0) + 0.5);
        let texels = (in.c0.z - in.c0.x) * f32(textureDimensions(img).x);
        let lod = max(log2(texels / max(half.x * 2.0, 1.0)), 0.0);
        col = textureSampleLevel(img, samp, uv, lod);
        if in.c1.y > 0.5 {
            // Template glyph: its alpha is the shape, the colour comes from the text colour.
            col = vec4f(in.c1.zw, in.p1.x, 1.0) * (in.p1.y * col.a);
        }
    } else {
        let dir = in.p1.xy;
        let extent = abs(dir.x) * half.x + abs(dir.y) * half.y;
        let t = clamp(dot(p, dir) / max(extent * 2.0, 0.001) + 0.5, 0.0, 1.0);
        col = premul(mix(in.c0, in.c1, t));
    }

    let bw = in.p0.y;
    if bw > 0.0 {
        let b = premul(in.border) * clamp(d + bw + 0.5, 0.0, 1.0);
        col = b + col * (1.0 - b.a);
    }
    if kind == 2.0 {
        // Glass is drawn without blending: it already contains the backdrop, and blending
        // it over a translucent scene (transparent window) would count that backdrop twice.
        // Its edge is blended here against the snapshot, which is what lies underneath.
        let k = cover * alpha;
        if k <= 0.0 {
            discard;
        }
        let under = textureSampleLevel(sharp, samp, in.xy / g.screen, 0.0);
        return mix(under, col, k);
    }
    return col * cover * alpha;
}
