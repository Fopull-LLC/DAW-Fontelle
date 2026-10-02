// Still Water: light moving slowly on deep, calm water — so slow it reads
// as "is that moving?".
//
// color0  the light on the water   color1  a second, cooler glint
// color2  the water                 color3  (unused)
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let t = bd.time * 0.12;
    let p = vec2<f32>(uv.x * aspect, uv.y) * 2.4 / bd.scale;
    // Two slow currents warp a third: the swell.
    let w = vec2<f32>(
        bd_fbm(p + vec2<f32>(t * 0.6, t * 0.2), 4),
        bd_fbm(p * 1.3 - vec2<f32>(t * 0.3, -t * 0.5) + 5.2, 4),
    );
    let n = bd_fbm(p * 1.6 + w * 1.6 + vec2<f32>(0.0, t * 0.4), 5);
    // Light gathers on the swell's crests, in thin lines.
    let ridge = 1.0 - abs(n * 2.0 - 1.0);
    var col = mix(bd.color2.rgb * 0.82, bd.color2.rgb * 1.12, 1.0 - uv.y);
    col = col + bd.color0.rgb * pow(ridge, 6.0) * 0.22;
    col = col + bd.color1.rgb * smoothstep(0.58, 0.95, w.x * n * 1.8) * 0.05;
    // A faint sheen from the top, as if a window were above.
    col = col + bd.color0.rgb * exp(-uv.y * 3.5) * 0.035;
    return vec4<f32>(col, 1.0);
}
