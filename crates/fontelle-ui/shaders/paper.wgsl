// Paper: the grain and fibres of a good sheet, drawn once. It does not
// move: the theme gives it speed 0.
//
// color0  the fibres   color2  the sheet
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let grain = (bd_hash(floor(px * 1.5)) - 0.5) * 0.018;
    let cloud = (bd_fbm(px / (90.0 * bd.scale), 5) - 0.5) * 0.08;
    // Fibres: long thin streaks, laid mostly one way.
    let q = bd_rot(0.35) * px / bd.scale;
    let fibre = smoothstep(0.82, 0.95, bd_noise(vec2<f32>(q.x / 60.0, q.y / 2.2)));
    var col = bd.color2.rgb * (1.0 + cloud + grain);
    col = mix(col, bd.color0.rgb, fibre * 0.12);
    return vec4<f32>(col, 1.0);
}
