// Waves: flowing contour lines, like a sound slowly moving through water.
//
// color0  the lines   color1  their glow   color2  the ground
// params0.x  how many lines (default 14)
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let n = select(14.0, bd.params0.x, bd.params0.x > 0.0);
    let p = vec2<f32>(uv.x * aspect, uv.y) / bd.scale;
    let t = bd.time * 0.08;
    let h = bd_fbm(p * 1.4 + vec2<f32>(t, t * 0.4), 4) + p.y * 0.8;
    let v = h * n;
    let d = abs(fract(v) - 0.5);
    let w = 1.2 * n * 1.4 / bd.resolution.y / bd.scale;
    let line = 1.0 - smoothstep(0.0, w + 0.02, d - 0.01);
    var col = bd.color2.rgb + bd.color1.rgb * smoothstep(0.5, 0.0, d) * 0.06;
    col = col + bd.color0.rgb * line * (0.35 + 0.25 * sin(v * 0.7 + t * 4.0));
    return vec4<f32>(col, 1.0);
}
