// Scanlines: a phosphor screen, glowing in the middle, with a slow roll and
// faint columns of characters drifting down in the dark.
//
// color0  the phosphor      color2  the dark glass
// params0.x  scanline spacing in points (default 3)
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let spacing = select(3.0, bd.params0.x, bd.params0.x > 0.0);
    let c = uv - vec2<f32>(0.5);
    let vignette = clamp(1.0 - dot(c, c) * 1.5, 0.0, 1.0);
    let glow = bd_fbm(uv * 2.5 + vec2<f32>(bd.time * 0.015, 0.0), 4);
    var col = bd.color2.rgb + bd.color0.rgb * (0.07 + 0.12 * glow) * vignette;

    // Columns of glyph-like cells falling slowly, each at its own pace.
    let cell = vec2<f32>(9.0, 14.0) * bd.scale;
    let colx = floor(px.x / cell.x);
    let speed = 0.6 + bd_hash(vec2<f32>(colx, 3.0)) * 1.4;
    let y = px.y / cell.y - bd.time * speed;
    let id = vec2<f32>(colx, floor(y));
    let lit = step(0.86, bd_hash(id));
    let head = fract(-y * 0.07 + bd_hash(vec2<f32>(colx, 9.0)));
    let f = fract(vec2<f32>(px.x / cell.x, y));
    let glyph = step(0.35, bd_hash(id * 3.7 + floor(f * vec2<f32>(3.0, 4.0))));
    col = col + bd.color0.rgb * lit * glyph * pow(head, 6.0) * 0.35 * vignette;

    let line = 0.5 + 0.5 * cos(px.y / spacing * 6.2831);
    col = col * (0.78 + 0.22 * line);
    let roll = fract(uv.y - bd.time * 0.04);
    col = col + bd.color0.rgb * exp(-abs(roll - 0.5) * 24.0) * 0.07;
    return vec4<f32>(col * (0.35 + 0.65 * vignette), 1.0);
}
