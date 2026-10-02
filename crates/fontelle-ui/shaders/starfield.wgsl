// Starfield: stars drifting slowly past, nearer ones faster.
//
// color2  space   color3  the stars   color0  a faint far haze
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    var col = bd.color2.rgb + bd.color0.rgb * bd_fbm(uv * 2.5, 4) * 0.08;
    for (var i = 0; i < 4; i = i + 1) {
        let fi = f32(i);
        let cell = (18.0 + fi * 16.0) * bd.scale;
        let g = px / cell + vec2<f32>(bd.time * (0.05 + fi * 0.05), fi * 5.3);
        let id = floor(g);
        let h = bd_hash(id + vec2<f32>(fi * 17.0));
        if (h < 0.12) {
            let off = vec2<f32>(bd_hash(id + 2.1), bd_hash(id + 5.9)) - vec2<f32>(0.5);
            let d = length(fract(g) - vec2<f32>(0.5) - off * 0.6) * cell;
            col = col + bd.color3.rgb * smoothstep(0.6 + fi * 0.4, 0.0, d) * (0.35 + fi * 0.2);
        }
    }
    return vec4<f32>(col, 1.0);
}
