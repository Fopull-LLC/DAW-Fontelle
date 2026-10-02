// Drift: soft pools of colour wandering past each other.
//
// color0, color1, color3  the three pools   color2  the ground they sit on
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let p = (uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0) / bd.scale;
    let t = bd.time * 0.06;
    var col = bd.color2.rgb;
    let c0 = vec2<f32>(sin(t * 1.3) * 0.6, cos(t * 0.9) * 0.35);
    let c1 = vec2<f32>(cos(t * 0.7 + 2.0) * 0.7, sin(t * 1.1 + 1.0) * 0.4);
    let c3 = vec2<f32>(sin(t * 0.5 + 4.0) * 0.5, cos(t * 1.4 + 3.0) * 0.45);
    let warp = vec2<f32>(bd_fbm(p * 2.0 + t, 3), bd_fbm(p * 2.0 - t, 3)) * 0.25;
    col = mix(col, bd.color0.rgb, exp(-dot(p + warp - c0, p + warp - c0) * 3.0) * 0.8);
    col = mix(col, bd.color1.rgb, exp(-dot(p + warp - c1, p + warp - c1) * 3.5) * 0.7);
    col = mix(col, bd.color3.rgb, exp(-dot(p + warp - c3, p + warp - c3) * 4.5) * 0.5);
    let grain = (bd_hash(px + vec2<f32>(t)) - 0.5) * 0.025;
    return vec4<f32>(col + grain, 1.0);
}
