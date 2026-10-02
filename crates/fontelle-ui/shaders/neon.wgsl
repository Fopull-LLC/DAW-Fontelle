// Neon Grid: a synthwave sunset over an endless neon floor that scrolls one
// line per bar of the song, and flashes on the beat.
//
// color0  the sun's lower half, the horizon glow   color1  the grid lines
// color2  the sky                                   color3  the sun's upper half
// params0.x  horizon height, 0-1 (default 0.6)
// Fontelle's twin of Floptle's builtin:grid, on bd.beat.
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let horizon = select(0.6, bd.params0.x, bd.params0.x > 0.0);
    let pulse = exp(-fract(bd.beat) * 4.0);
    var col = vec3<f32>(0.0);
    if (uv.y < horizon) {
        let k = uv.y / horizon;
        col = mix(bd.color2.rgb * 0.7, mix(bd.color2.rgb, bd.color0.rgb, 0.45), pow(k, 2.2));
        let sr = 0.2 * bd.scale;
        let sp = vec2<f32>((uv.x - 0.5) * aspect, uv.y - horizon + sr * 0.35);
        let d = length(sp);
        if (d < sr) {
            let g = clamp((uv.y - (horizon - sr * 1.35)) / sr, 0.0, 1.0);
            let band = fract(uv.y * 70.0 / bd.scale - bd.time * 0.15);
            let cut = step(band, g * 0.55) * step(0.35, g);
            let sun = mix(bd.color3.rgb, bd.color0.rgb, g);
            col = mix(sun, col, cut);
        }
        col = col + bd.color0.rgb * exp(-max(d - sr, 0.0) * 9.0) * 0.35;
        col = col + bd.color0.rgb * exp(-(horizon - uv.y) * 22.0) * (0.4 + pulse * 0.15);
    } else {
        let z = max(uv.y - horizon, 0.0005);
        let depth = 0.3 / z;
        let gx = (uv.x - 0.5) * aspect * depth * 2.2;
        // One floor line a bar, at four beats to the bar.
        let gz = depth + bd.beat * 0.25;
        let wx = 1.1 * aspect * depth * 2.2 / bd.resolution.x;
        let wz = 1.1 * 0.3 / (z * z) / bd.resolution.y;
        let lx = 1.0 - smoothstep(0.0, wx, 0.5 - abs(fract(gx) - 0.5));
        let lz = 1.0 - smoothstep(0.0, wz, 0.5 - abs(fract(gz) - 0.5));
        let fade = smoothstep(0.0, 0.12, z);
        col = bd.color2.rgb * 0.5 + bd.color0.rgb * exp(-z * 14.0) * 0.45;
        let glow = max(lx, lz * smoothstep(0.0, 0.02, z));
        let near = 1.0 - smoothstep(0.05, 0.4, z) * 0.6;
        col = col + bd.color1.rgb * glow * fade * (0.6 + pulse * 0.25) * near;
    }
    return vec4<f32>(col, 1.0);
}
