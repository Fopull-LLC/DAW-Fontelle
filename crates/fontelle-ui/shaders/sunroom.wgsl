// Sunroom: an afternoon. Sun comes in through a window on the right and
// lands on the wall as a slanting patch of light, crossed by the window's
// frame, with the shadows of leaves outside swaying in it.
//
// Before, the leaves were all there was, and soft enough to read as clouds:
// the window is what makes it a room with the sun in it.
//
// color0  the leaves' shadow   color1  sunlight
// color2  the wall              color3  (unused)
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let q = vec2<f32>(uv.x * aspect, uv.y);
    let t = bd.time * 0.05;

    // The patch of sun: the window's shape, slanted as the light comes down.
    let s = vec2<f32>(q.x + q.y * 0.42, q.y);
    let x0 = aspect * 0.52;
    let x1 = aspect * 1.08;
    let soft = 0.018;
    let sunpatch = smoothstep(x0, x0 + soft, s.x) * smoothstep(x1, x1 - soft, s.x)
        * smoothstep(0.02, 0.02 + soft, s.y) * smoothstep(0.96, 0.96 - soft, s.y);
    // The frame: one upright down the middle, one bar across, as soft
    // shadows — the sun is not a point.
    let u = (s.x - x0) / (x1 - x0);
    let mull = smoothstep(0.022, 0.010, abs(u - 0.5)) + smoothstep(0.016, 0.006, abs(s.y - 0.46));
    let pane = sunpatch * (1.0 - clamp(mull, 0.0, 1.0));

    // Leaves outside: clusters swaying as a whole, their edges fluttering.
    let p = q * 3.2 / max(bd.scale, 0.001);
    let sway = vec2<f32>(sin(t * 1.7) * 0.10 + sin(t * 4.3) * 0.02, cos(t * 1.3) * 0.07);
    let a = bd_fbm(p + sway + vec2<f32>(t * 0.12, 0.0), 5);
    let b = bd_fbm(p * 2.4 - sway * 1.8 + vec2<f32>(4.0, 1.0), 4);
    let canopy = a * 0.70 + b * 0.44;
    let leaf = smoothstep(0.62, 0.68, canopy) * (0.55 + 0.45 * smoothstep(0.0, 1.0, u));

    // The wall in the room's own light, warmer near the window.
    var col = bd.color2.rgb * (0.90 + 0.06 * uv.x);
    let lit = pane * (1.0 - leaf * 0.70);
    col = col + bd.color1.rgb * lit * 0.24;
    col = mix(col, col * 0.93 + bd.color0.rgb * 0.04, leaf * pane * 0.5);
    // The glow the patch throws round itself.
    col = col + bd.color1.rgb * 0.05 * smoothstep(x0 - 0.3, x0 + 0.2, s.x);
    return vec4<f32>(col, 1.0);
}
