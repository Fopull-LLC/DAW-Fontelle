// Bubblegum: soft pastel blobs drifting and merging, squashing gently on
// the beat.
//
// color0, color1, color3  the blobs   color2  the ground
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let t = bd.time * 0.08;
    let pulse = exp(-fract(bd.beat) * 6.0);
    var p = vec2<f32>(uv.x * aspect, uv.y) / bd.scale;
    // The squash: a touch wider and shorter for a moment on each beat.
    p = vec2<f32>(p.x * (1.0 - pulse * 0.025), p.y * (1.0 + pulse * 0.04));
    var field = 0.0;
    var tint = vec3<f32>(0.0);
    for (var i = 0; i < 6; i = i + 1) {
        let fi = f32(i);
        let c = vec2<f32>(
            (0.5 + 0.42 * sin(t * (0.7 + fi * 0.13) + fi * 1.9)) * aspect / bd.scale,
            (0.5 + 0.38 * cos(t * (0.6 + fi * 0.11) + fi * 2.7)) / bd.scale,
        );
        let r = 0.16 + 0.06 * sin(fi * 3.1);
        let d = p - c;
        let v = r * r / max(dot(d, d), 1e-4);
        field = field + v;
        var c3 = bd.color0.rgb;
        if (i % 3 == 1) {
            c3 = bd.color1.rgb;
        } else if (i % 3 == 2) {
            c3 = bd.color3.rgb;
        }
        tint = tint + c3 * v;
    }
    tint = tint / max(field, 1e-4);
    let body = smoothstep(0.85, 1.15, field);
    // A soft highlight toward the upper left of each blob's body.
    let rim = smoothstep(1.15, 2.4, field);
    var col = bd.color2.rgb;
    col = mix(col, tint, body * 0.55);
    col = col + vec3<f32>(1.0) * rim * 0.06;
    // Paper-soft grain so the pastel never bands.
    col = col + (bd_hash(px) - 0.5) * 0.012;
    return vec4<f32>(col, 1.0);
}
