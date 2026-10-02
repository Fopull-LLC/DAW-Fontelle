// Midnight: a cold, clear night. Faint stars, and a thin blue mist moving
// slowly across them.
//
// color0  the mist   color1  a second, deeper blue
// color2  the night  color3  the stars
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let t = bd.time * 0.03;
    let p = vec2<f32>(uv.x * aspect, uv.y);
    var col = mix(bd.color2.rgb * 1.2, bd.color2.rgb * 0.8, uv.y);
    // Mist in two bands, crossing each other slowly.
    let m0 = bd_fbm(p * vec2<f32>(1.2, 3.0) + vec2<f32>(t, 0.0), 5);
    let m1 = bd_fbm(p * vec2<f32>(2.0, 4.5) - vec2<f32>(t * 1.4, t * 0.2) + 9.0, 4);
    col = col + bd.color0.rgb * smoothstep(0.45, 0.85, m0) * 0.16;
    col = col + bd.color1.rgb * smoothstep(0.5, 0.9, m1) * 0.13;
    // Stars, still and faint, a few twinkling.
    let cell = 22.0 * bd.scale;
    let g = px / cell;
    let id = floor(g);
    let h = bd_hash(id + vec2<f32>(3.0, 1.0));
    if (h < 0.12) {
        let off = vec2<f32>(bd_hash(id + 1.1), bd_hash(id + 4.7)) - vec2<f32>(0.5);
        let d = length(fract(g) - vec2<f32>(0.5) - off * 0.6) * cell;
        let tw = 0.65 + 0.35 * sin(bd.time * (0.3 + h * 4.0) + h * 70.0);
        col = col + bd.color3.rgb * smoothstep(1.2, 0.0, d) * tw * 0.85 * (1.0 - uv.y * 0.5);
    }
    return vec4<f32>(col, 1.0);
}
