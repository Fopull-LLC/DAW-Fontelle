// Aurora: curtains of light folding slowly across a night sky.
//
// color0  the bright lower edge      color1  the fading upper hue
// color2  the sky                    color3  stars
// params0.x  how high the curtains hang, 0-1 down the window (default 0.42)
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let t = bd.time * 0.05;
    var col = mix(bd.color2.rgb * 0.6, bd.color2.rgb * 1.4, uv.y);
    let base = select(0.42, bd.params0.x, bd.params0.x > 0.0);
    let x0 = uv.x * aspect / bd.scale;
    for (var i = 0; i < 3; i = i + 1) {
        let fi = f32(i);
        // The curtain's fold: a slow wave plus noise, different per layer.
        let fold = sin(x0 * (1.3 + fi * 0.6) + t * (0.8 + fi * 0.3) + fi * 2.1) * 0.08
            + (bd_fbm(vec2<f32>(x0 * 1.4 + fi * 5.0, t * 0.5 + fi), 4) - 0.5) * 0.22;
        let edge = base + fi * 0.07 + fold;
        let d = edge - uv.y;
        // Bright at the hem, rays rising above it, nothing below.
        var band = 0.0;
        if (d > 0.0) {
            band = exp(-d * (3.2 + fi * 1.5));
        } else {
            band = exp(d * 60.0);
        }
        let rays = 0.45 + 0.55 * pow(bd_noise(vec2<f32>(x0 * 55.0 + fi * 11.0 + fold * 40.0, t * 2.0)), 1.5);
        let hue = mix(bd.color0.rgb, bd.color1.rgb, clamp(d * 2.6, 0.0, 1.0));
        let shimmer = 0.75 + 0.25 * sin(x0 * 9.0 - t * 6.0 + fi);
        col = col + hue * band * rays * shimmer * (0.75 - fi * 0.18);
    }
    // A faint glow on the horizon below, and stars above.
    col = col + bd.color0.rgb * exp(-abs(uv.y - 0.95) * 8.0) * 0.06;
    let g = px / 26.0;
    let id = floor(g);
    let h = bd_hash(id);
    if (h < 0.1) {
        let off = vec2<f32>(bd_hash(id + 1.3), bd_hash(id + 2.9)) - vec2<f32>(0.5);
        let d = length(fract(g) - vec2<f32>(0.5) - off * 0.6) * 26.0;
        col = col + bd.color3.rgb * smoothstep(1.3, 0.0, d) * (1.0 - uv.y) * 0.7;
    }
    return vec4<f32>(col, 1.0);
}
