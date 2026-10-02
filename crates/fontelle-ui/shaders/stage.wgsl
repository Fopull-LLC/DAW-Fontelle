// Stage: a concert stage at night. A hex-lit floor, two long ribbons of
// light sweeping across, notes drifting up, sparkles, and an equalizer
// breathing along the bottom on the beat.
//
// color0  the first light (teal)   color1  the second light (pink)
// color2  the dark stage           color3  white light
// params0.x  beats per minute; 0 follows the song (bd.beat)
// Fontelle reads bd.beat and bd.level, so the floor and the equalizer move
// with the music while it plays.

fn sd_box(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

// An eighth note, head at the origin, stem rising (screen y points down).
fn sd_note(q: vec2<f32>) -> f32 {
    let hr = bd_rot(0.45) * q;
    let head = length(vec2<f32>(hr.x / 1.3, hr.y)) - 0.24;
    let stem = sd_box(q - vec2<f32>(0.28, -0.55), vec2<f32>(0.045, 0.55));
    let fq = bd_rot(-0.85) * (q - vec2<f32>(0.47, -0.92));
    let flag = sd_box(fq, vec2<f32>(0.22, 0.05));
    return min(head, min(stem, flag));
}

fn hex_dist(p: vec2<f32>) -> f32 {
    let q = abs(p);
    return max(dot(q, vec2<f32>(0.8660254, 0.5)), q.y);
}

fn wmod(x: vec2<f32>, y: vec2<f32>) -> vec2<f32> {
    return x - y * floor(x / y);
}

fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let t = bd.time;
    // The song's beat, or a tempo of the theme's own.
    let beats = select(bd.beat, t * bd.params0.x / 60.0, bd.params0.x > 0.0);
    let beat = fract(beats);
    let pulse = exp(-beat * 5.0);
    let teal = bd.color0.rgb;
    let pink = bd.color1.rgb;
    let white = bd.color3.rgb;

    // The stage: dark, lifting toward the lower left where the lights are.
    var col = bd.color2.rgb * (0.75 + 0.5 * (1.0 - uv.y));
    col = col + teal * exp(-length((uv - vec2<f32>(0.1, 1.05)) * vec2<f32>(aspect, 1.0)) * 1.8) * 0.28;
    col = col + pink * exp(-length((uv - vec2<f32>(0.95, -0.05)) * vec2<f32>(aspect, 1.0)) * 2.2) * 0.16;

    // Hex floor of light: faint lines, a wave of brightness rolling across,
    // a few cells lit on each beat.
    let hs = 30.0 * bd.scale;
    let hp = px / hs;
    let r = vec2<f32>(1.0, 1.7320508);
    let h = r * 0.5;
    let a = wmod(hp, r) - h;
    let b = wmod(hp - h, r) - h;
    var gv = b;
    if (dot(a, a) < dot(b, b)) {
        gv = a;
    }
    let id = hp - gv;
    let edge = 0.5 - hex_dist(gv);
    let line = 1.0 - smoothstep(0.0, 0.05, edge);
    let wave = 0.5 + 0.5 * sin(dot(id, vec2<f32>(0.35, 0.22)) - t * 1.6);
    let lit = step(0.93, bd_hash(id + floor(beats) * 0.17));
    col = col + teal * line * (0.035 + 0.06 * wave * wave);
    col = col + teal * lit * smoothstep(0.5, 0.0, hex_dist(gv)) * 0.10 * (0.4 + pulse);

    // Two ribbons of light sweeping across.
    for (var i = 0; i < 2; i = i + 1) {
        let fi = f32(i);
        let x = uv.x * aspect;
        let y = 0.34 + fi * 0.2 + 0.11 * sin(x * (1.6 - fi * 0.3) + t * (0.35 + fi * 0.1) + fi * 2.0)
            + 0.04 * sin(x * 4.3 - t * 0.7 + fi);
        let d = abs(uv.y - y);
        let width = 0.010 + 0.008 * sin(x * 2.0 + t * 0.5 + fi * 3.0);
        let core = exp(-d * d / (width * width) * 0.6);
        let halo = exp(-d * 18.0);
        let c = mix(teal, pink, fi);
        let shimmer = 0.75 + 0.25 * sin(x * 14.0 - t * 2.5 + fi * 5.0);
        col = col + c * (core * 0.55 * shimmer + halo * 0.12) + white * core * 0.12;
    }

    // Notes drifting upward, each cell its own note at its own pace.
    let cell = 150.0 * bd.scale;
    let g = px / cell;
    let cid = floor(g);
    let hn = bd_hash(cid + vec2<f32>(7.0, 3.0));
    if (hn < 0.42) {
        let rise = fract(t * (0.02 + hn * 0.05) + hn * 7.0);
        let center = vec2<f32>(0.25 + 0.4 * bd_hash(cid + 1.9), 0.9 - rise * 0.5);
        let size = 0.13 + 0.08 * bd_hash(cid + 4.4);
        let sway = vec2<f32>(sin(t * 0.8 + hn * 20.0) * 0.06, 0.0);
        let q = (fract(g) - center - sway) / size;
        let d = sd_note(bd_rot(sin(t + hn * 9.0) * 0.25) * q) * size;
        let fade = smoothstep(0.0, 0.2, rise) * smoothstep(1.0, 0.7, rise);
        let nc = mix(teal, pink, step(0.5, bd_hash(cid + 8.8)));
        col = col + nc * smoothstep(0.012, 0.0, d) * 0.55 * fade;
        col = col + nc * exp(-max(d, 0.0) * 38.0) * 0.22 * fade;
    }

    // Sparkles: four-pointed, twinkling.
    let sg = px / 46.0;
    let sid = floor(sg);
    let hs2 = bd_hash(sid + vec2<f32>(2.0, 9.0));
    if (hs2 < 0.07) {
        let sp = (fract(sg) - vec2<f32>(0.5) - (vec2<f32>(bd_hash(sid + 3.1), bd_hash(sid + 6.7)) - 0.5) * 0.6) * 46.0;
        let tw = pow(0.5 + 0.5 * sin(t * (1.5 + hs2 * 20.0) + hs2 * 60.0), 6.0);
        let star = smoothstep(1.4, 0.0, abs(sp.x)) * smoothstep(9.0, 0.0, abs(sp.y))
            + smoothstep(1.4, 0.0, abs(sp.y)) * smoothstep(9.0, 0.0, abs(sp.x));
        col = col + mix(white, teal, 0.3) * star * tw * 0.7;
    }

    // The equalizer, breathing along the bottom on the beat — and with the
    // master meter, while there is sound.
    let barw = 11.0 * bd.scale;
    let col_i = floor(px.x / barw);
    let inbar = step(0.22, fract(px.x / barw));
    let loud = 0.55 + 0.45 * max(pulse, bd.level);
    let level = (0.25 + 0.75 * pow(bd_noise(vec2<f32>(col_i * 0.37, t * 1.7)), 1.5)) * loud;
    let env = 0.6 + 0.4 * sin(col_i * 0.09 + t * 0.3);
    let hgt = level * env * 0.2;
    let from_bottom = 1.0 - uv.y;
    if (from_bottom < hgt) {
        let k = from_bottom / max(hgt, 1e-3);
        let seg = step(0.3, fract(from_bottom * bd.window.y / 5.0));
        col = col + mix(teal, pink, k * k) * inbar * seg * (0.22 + 0.25 * k);
    }
    col = col + teal * exp(-from_bottom * 30.0) * 0.12 * (0.5 + pulse);

    return vec4<f32>(col, 1.0);
}
