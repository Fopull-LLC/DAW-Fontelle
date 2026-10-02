// Phosphor: a green CRT. Scanlines, a slow roll, faint characters falling
// in the dark, and an oscilloscope's Lissajous trace whose ratio drifts —
// and swells with the master meter.
//
// color0  the phosphor   color2  the dark glass
// params0.x  scanline spacing in points (default 3)
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let spacing = select(3.0, bd.params0.x, bd.params0.x > 0.0);
    let c = uv - vec2<f32>(0.5);
    let vignette = clamp(1.0 - dot(c, c) * 1.5, 0.0, 1.0);
    let glow = bd_fbm(uv * 2.5 + vec2<f32>(bd.time * 0.015, 0.0), 4);
    var col = bd.color2.rgb + bd.color0.rgb * (0.06 + 0.10 * glow) * vignette;

    // Columns of glyph-like cells falling slowly, each at its own pace.
    let cell = vec2<f32>(9.0, 14.0) * bd.scale;
    let colx = floor(px.x / cell.x);
    let speed = 0.6 + bd_hash(vec2<f32>(colx, 3.0)) * 1.4;
    let y = px.y / cell.y - bd.time * speed;
    let id = vec2<f32>(colx, floor(y));
    let lit = step(0.88, bd_hash(id));
    let head = fract(-y * 0.07 + bd_hash(vec2<f32>(colx, 9.0)));
    let f = fract(vec2<f32>(px.x / cell.x, y));
    let glyph = step(0.35, bd_hash(id * 3.7 + floor(f * vec2<f32>(3.0, 4.0))));
    col = col + bd.color0.rgb * lit * glyph * pow(head, 6.0) * 0.35 * vignette;

    // The trace: x = sin(a t + phase), y = sin(b t), its ratio drifting.
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let q = (uv - vec2<f32>(0.62, 0.5)) * vec2<f32>(aspect, 1.0);
    let amp = 0.22 + 0.08 * bd.level;
    let ra = 3.0 + 0.5 * sin(bd.time * 0.021);
    let rb = 2.0;
    let phase = bd.time * 0.17;
    var dmin = 10.0;
    var prev = vec2<f32>(sin(phase), 0.0) * amp;
    for (var i = 1; i <= 64; i = i + 1) {
        let s = f32(i) / 64.0 * 6.2831853;
        let p = vec2<f32>(sin(ra * s + phase) * 1.3, sin(rb * s)) * amp;
        let e = p - prev;
        let h = clamp(dot(q - prev, e) / max(dot(e, e), 1e-6), 0.0, 1.0);
        dmin = min(dmin, length(q - prev - e * h));
        prev = p;
    }
    let px_size = 1.0 / max(bd.resolution.y, 1.0);
    col = col + bd.color0.rgb * (smoothstep(px_size * 2.5, 0.0, dmin) * 0.7 + exp(-dmin * 70.0) * 0.18);

    let line = 0.5 + 0.5 * cos(px.y / spacing * 6.2831);
    col = col * (0.8 + 0.2 * line);
    let roll = fract(uv.y - bd.time * 0.04);
    col = col + bd.color0.rgb * exp(-abs(roll - 0.5) * 24.0) * 0.05;
    return vec4<f32>(col * (0.4 + 0.6 * vignette), 1.0);
}
