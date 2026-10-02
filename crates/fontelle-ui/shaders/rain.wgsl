// Lo-fi Rain: a city at night through a rainy window. Out-of-focus lights
// drift slowly behind the glass, drops sit on it bending the lights behind
// them, and rain runs down it.
//
// color0  warm street lights   color1  neon signs   color3  cold lights
// color2  the night
fn rain_lights(p: vec2<f32>, t: f32) -> vec3<f32> {
    var col = vec3<f32>(0.0);
    for (var i = 0; i < 2; i = i + 1) {
        let fi = f32(i);
        // Two depths: the far one smaller and slower.
        let cell = (70.0 + fi * 60.0) * bd.scale;
        let g = (p + vec2<f32>(t * (4.0 + fi * 6.0), fi * 37.0)) / cell;
        let id = floor(g);
        let h = bd_hash(id + vec2<f32>(fi * 13.0, 3.0));
        if (h < 0.45) {
            let off = vec2<f32>(bd_hash(id + 1.7), bd_hash(id + 5.1)) - vec2<f32>(0.5);
            let d = length(fract(g) - vec2<f32>(0.5) - off * 0.5) * cell;
            let r = cell * (0.16 + 0.14 * bd_hash(id + 9.3));
            let disk = smoothstep(r, r * 0.55, d);
            // The rim of a bokeh disk is a touch brighter than its middle.
            let rim = smoothstep(r * 0.65, r * 0.95, d) * disk;
            let k = bd_hash(id + 2.2);
            var c = bd.color0.rgb;
            if (k > 0.66) {
                c = bd.color1.rgb;
            } else if (k > 0.4) {
                c = bd.color3.rgb;
            }
            let flicker = 0.85 + 0.15 * sin(t * (0.5 + h * 3.0) + h * 30.0);
            col = col + c * (disk * 0.40 + rim * 0.14) * (0.55 + fi * 0.45) * flicker;
        }
    }
    return col;
}

fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let t = bd.time;
    // Drops resting on the glass: each bends what is behind it.
    let dcell = 34.0 * bd.scale;
    let dg = px / dcell;
    let did = floor(dg);
    let dh = bd_hash(did + vec2<f32>(4.0, 8.0));
    var bend = vec2<f32>(0.0);
    var drop = 0.0;
    if (dh < 0.3) {
        let c = vec2<f32>(bd_hash(did + 0.7), bd_hash(did + 3.3)) * 0.6 + vec2<f32>(0.2);
        let v = fract(dg) - c;
        let r = 0.10 + 0.12 * bd_hash(did + 6.1);
        let d = length(v);
        drop = smoothstep(r, r * 0.8, d);
        bend = -v * drop * dcell * 1.6;
    }
    // Rain running down: a streak in some columns, falling at its own pace.
    let sw = 9.0 * bd.scale;
    let colx = floor(px.x / sw);
    let speed = 0.25 + bd_hash(vec2<f32>(colx, 1.0)) * 0.35;
    let y = uv.y - t * speed - bd_hash(vec2<f32>(colx, 7.0)) * 10.0;
    let seg = fract(y * 0.7);
    let has = step(0.82, bd_hash(vec2<f32>(colx, floor(y * 0.7))));
    let inx = smoothstep(0.5, 0.1, abs(fract(px.x / sw) - 0.5));
    let streak = has * inx * smoothstep(0.0, 0.06, seg) * smoothstep(0.3, 0.06, seg);

    var col = mix(bd.color2.rgb * 1.25, bd.color2.rgb * 0.7, uv.y * 0.6 + 0.2);
    col = col + rain_lights(px + bend + vec2<f32>(0.0, streak * 4.0), t);
    // A drop catches light along its lower edge.
    col = col + (bd.color3.rgb * 0.08 + bd.color0.rgb * 0.04) * drop * smoothstep(0.0, 0.5, fract(dg.y));
    col = col + mix(bd.color3.rgb, vec3<f32>(1.0), 0.5) * streak * 0.12;
    return vec4<f32>(col, 1.0);
}
