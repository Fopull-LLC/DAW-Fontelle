// Sunroom: daylight on a warm wall, with the shadows of leaves drifting
// over it in a slow breeze.
//
// color0  the leaves' shadow   color1  sunlight
// color2  the wall              color3  (unused)
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let t = bd.time * 0.05;
    let p = px / (240.0 * bd.scale);
    // The breeze: the canopy sways as a whole, and its edges flutter.
    let sway = vec2<f32>(sin(t * 1.7) * 0.18 + sin(t * 4.1) * 0.03, cos(t * 1.3) * 0.12);
    let a = bd_fbm(p + sway + vec2<f32>(t * 0.15, 0.0), 5);
    let b = bd_fbm(p * 2.6 - sway * 1.6 + vec2<f32>(4.0, 1.0), 4);
    let canopy = a * 0.72 + b * 0.42;
    // Leaves cast soft-edged shadow; the gaps between are sun.
    let shade = smoothstep(0.50, 0.62, canopy);
    let sun = smoothstep(0.46, 0.30, canopy);
    // Stronger toward the top right, where the window is.
    let reach = clamp(0.35 + 0.65 * (uv.x * 0.6 + (1.0 - uv.y) * 0.6), 0.0, 1.0);
    var col = bd.color2.rgb;
    col = mix(col, col * 0.86 + bd.color0.rgb * 0.06, shade * reach * 0.9);
    col = col + bd.color1.rgb * sun * reach * 0.07;
    return vec4<f32>(col, 1.0);
}
