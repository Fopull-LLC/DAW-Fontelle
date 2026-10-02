// Embers: a fire seen low. Smoke curling up out of the dark, embers rising
// on the heat and swaying as they cool, and the glow of the coals along
// the bottom, shimmering.
//
// color0  the fire's glow   color1  the embers   color2  the dark
// color3  the hottest light
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let t = bd.time;
    let p = vec2<f32>(uv.x * aspect, uv.y);
    let from_bottom = 1.0 - uv.y;

    // Smoke: warped noise drifting upward, thinning as it rises.
    let rise = vec2<f32>(0.0, t * 0.06);
    let warp = vec2<f32>(bd_fbm(p * 1.8 + rise, 3), bd_fbm(p * 1.8 + rise + 7.3, 3));
    let smoke = bd_fbm(p * 2.2 + warp * 1.4 + rise * 1.6, 5);
    var col = bd.color2.rgb * (0.85 + 0.3 * from_bottom);
    col = col + bd.color0.rgb * smoothstep(0.4, 0.85, smoke) * 0.16 * (0.35 + from_bottom);

    // The coals' heat along the bottom, shimmering.
    let shimmer = bd_noise(vec2<f32>(uv.x * aspect * 8.0, t * 1.3)) * 0.5 + 0.5;
    col = col + bd.color0.rgb * exp(-from_bottom * 6.0) * (0.30 + 0.12 * shimmer);
    col = col + bd.color3.rgb * exp(-from_bottom * 22.0) * 0.18 * shimmer;

    // Embers: one in some cells, rising and swaying, flickering as it cools.
    for (var i = 0; i < 2; i = i + 1) {
        let fi = f32(i);
        let cell = (64.0 + fi * 40.0) * bd.scale;
        let speed = 18.0 + fi * 14.0;
        let g = vec2<f32>(px.x, px.y + t * speed) / cell;
        let id = floor(g);
        let h = bd_hash(id + vec2<f32>(fi * 9.0, 1.0));
        if (h < 0.35) {
            let sway = sin(t * (0.6 + h) + id.y * 1.7) * 0.18;
            let c = vec2<f32>(0.5 + (bd_hash(id + 2.3) - 0.5) * 0.6 + sway, 0.5);
            let d = length((fract(g) - c) * vec2<f32>(1.0, 0.8)) * cell;
            let size = 1.2 + fi * 0.6 + h * 1.5;
            let flicker = 0.55 + 0.45 * sin(t * (4.0 + h * 9.0) + h * 50.0);
            // They cool as they climb.
            let heat = smoothstep(0.0, 0.9, from_bottom + 0.2) ;
            let life = 1.0 - heat * 0.75;
            let core = smoothstep(size, 0.0, d);
            let halo = exp(-d / (size * 3.0)) * 0.35;
            col = col + mix(bd.color1.rgb, bd.color3.rgb, core * 0.5) * (core + halo) * flicker * life * 1.1;
        }
    }
    return vec4<f32>(col, 1.0);
}
