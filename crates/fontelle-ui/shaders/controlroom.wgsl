// Control Room: a studio after dark. Acoustic foam on the wall, warm lamp
// pools breathing slowly across it, and a VU-amber glow at the foot that
// rises with the master meter.
//
// color0  the lamps (amber)   color1  a cooler second lamp
// color2  the room            color3  (unused)
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let t = bd.time;
    // Foam: a grid of pyramids, each lit from the upper left. Alternate
    // tiles turn a quarter, as foam does.
    let cell = 28.0 * bd.scale;
    let g = px / cell;
    let id = floor(g);
    var f = fract(g) - vec2<f32>(0.5);
    if ((i32(id.x) + i32(id.y)) % 2 == 0) {
        f = vec2<f32>(f.y, -f.x);
    }
    var n = vec2<f32>(0.0, sign(f.y));
    if (abs(f.x) > abs(f.y)) {
        n = vec2<f32>(sign(f.x), 0.0);
    }
    let lightdir = normalize(vec2<f32>(-0.6, -0.8));
    let facet = dot(n, lightdir);
    var col = bd.color2.rgb * (1.0 + facet * 0.16 + (bd_hash(id) - 0.5) * 0.05);
    // Two lamp pools, drifting slowly and breathing.
    let q = vec2<f32>(uv.x * aspect, uv.y);
    let l0 = vec2<f32>(0.22 * aspect + sin(t * 0.05) * 0.08, 0.30 + cos(t * 0.04) * 0.05);
    let l1 = vec2<f32>(0.78 * aspect + cos(t * 0.045) * 0.08, 0.42 + sin(t * 0.035) * 0.06);
    let b0 = 0.75 + 0.25 * sin(t * 0.31);
    let b1 = 0.75 + 0.25 * sin(t * 0.23 + 2.0);
    col = col + bd.color0.rgb * exp(-dot(q - l0, q - l0) * 5.5) * 0.42 * b0 * (1.0 + facet * 0.5);
    col = col + bd.color1.rgb * exp(-dot(q - l1, q - l1) * 7.0) * 0.22 * b1 * (1.0 + facet * 0.5);
    // The meter's glow along the foot of the wall.
    let foot = 1.0 - uv.y;
    col = col + bd.color0.rgb * exp(-foot * (9.0 - bd.level * 5.0)) * (0.10 + bd.level * 0.30);
    return vec4<f32>(col, 1.0);
}
