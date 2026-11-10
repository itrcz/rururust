// One instanced quad draws every shape: fills, gradients, borders, shadows and glass.

struct Globals {
    screen: vec2f, // physical pixels
    scale: f32,    // points -> pixels
    _pad: f32,
}
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var backdrop: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct Instance {
    @location(0) rect: vec4f,
    @location(1) clip: vec4f,
    @location(2) c0: vec4f,
    @location(3) c1: vec4f,
    @location(4) border: vec4f,
    @location(5) p0: vec4f, // radius, border width, shadow sigma, kind
    @location(6) p1: vec4f, // gradient dir, clip radius, opacity
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
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, in: Instance) -> VOut {
    let s = g.scale;
    let corner = vec2f(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let r = in.rect * s;
    let grow = 1.0 + in.p0.z * s * 3.0; // room for AA and shadow falloff
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

    let depth = max(in.c1.y * g.scale, 1.0); // glass only: bevel width in px
    let dg = sdg_shape(p, half, radius, select(1.0, depth * 0.35, kind == 2.0));
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
        // Glass. The pane is flat in the middle and bevelled at the rim; the backdrop is
        // refracted through that bevel (index 1.5), slightly differently per colour channel.
        let nrm = dg.yz;
        let t = clamp(-d / depth, 0.0, 1.0);
        var bg: vec3f;
        if t < 1.0 {
            let slope = bevel_slope(t);
            let disp = in.c1.w * 0.25;
            let off = nrm * depth / g.screen;
            let uv = in.xy / g.screen;
            if disp > 0.0 {
                bg = vec3f(
                    textureSampleLevel(backdrop, samp, uv - off * refract_offset(slope, 1.5 - disp), 0.0).r,
                    textureSampleLevel(backdrop, samp, uv - off * refract_offset(slope, 1.5), 0.0).g,
                    textureSampleLevel(backdrop, samp, uv - off * refract_offset(slope, 1.5 + disp), 0.0).b,
                );
            } else {
                bg = textureSampleLevel(backdrop, samp, uv - off * refract_offset(slope, 1.5), 0.0).rgb;
            }
        } else {
            bg = textureSampleLevel(backdrop, samp, in.xy / g.screen, 0.0).rgb;
        }
        let luma = dot(bg, vec3f(0.2126, 0.7152, 0.0722));
        bg = mix(vec3f(luma), bg, in.c1.x);
        bg = mix(bg, in.c0.rgb, in.c0.a);

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
        col = vec4f(clamp(bg, vec3f(0.0), vec3f(1.0)), 1.0);
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
    return col * cover * alpha;
}
