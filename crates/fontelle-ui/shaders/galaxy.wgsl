// Galaxy: a spiral galaxy, tilted toward you and turning slowly, in a sky of
// faint nebulae and stars.
//
// color0  the outer arms        color1  star-forming knots and the haze
// color2  deep space            color3  the core and the stars
// params0.x  star density (default 1)
// params0.y  brightness (default 1)
// params0.z, params0.w  where the core sits, 0-1 across and down
//            (default 0.66, 0.42)
fn galaxy_arms(theta: f32, r: f32, t: f32, wobble: f32) -> f32 {
    // Two logarithmic arms: the angle that is "on an arm" winds outward.
    let phase = 2.0 * (theta - log(max(r, 0.02)) * 2.4 + t) + wobble;
    return 0.5 + 0.5 * cos(phase);
}

fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let cx = select(0.66, bd.params0.z, bd.params0.z > 0.0);
    let cy = select(0.42, bd.params0.w, bd.params0.w > 0.0);
    let bright = select(1.0, bd.params0.y, bd.params0.y > 0.0);
    let t = bd.time * 0.012;

    // Screen to the galaxy's own plane: rotate, then undo the tilt.
    var q = (uv - vec2<f32>(cx, cy)) * vec2<f32>(aspect, 1.0) / (0.75 * bd.scale);
    q = bd_rot(0.45) * q;
    q.y = q.y / 0.5;
    let r = length(q);
    let theta = atan2(q.y, q.x);

    // The sky behind it: big soft clouds in the two hues.
    let sky_n = bd_fbm(uv * vec2<f32>(aspect, 1.0) * 1.6 + vec2<f32>(t * 2.0, 0.0), 4);
    let sky_m = bd_fbm(uv * vec2<f32>(aspect, 1.0) * 3.1 - vec2<f32>(0.0, t * 3.0), 3);
    var col = bd.color2.rgb;
    col = col + bd.color0.rgb * smoothstep(0.45, 0.95, sky_n) * 0.22;
    col = col + bd.color1.rgb * smoothstep(0.6, 1.0, sky_m) * sky_n * 0.16;

    // The disc.
    let detail = bd_fbm(q * 3.0 + vec2<f32>(t * 4.0, 0.0), 4);
    let wobble = (detail - 0.5) * 2.2;
    let arms = pow(galaxy_arms(theta, r, t * 6.0, wobble), 3.0);
    let lanes = pow(galaxy_arms(theta, r, t * 6.0, wobble + 0.75), 8.0);
    let disc = exp(-r * 1.55);
    var glow = disc * (0.12 + arms * 0.9) * (0.45 + detail * 1.0);
    glow = glow * (1.0 - lanes * 0.55 * smoothstep(0.08, 0.35, r));

    // Warm in the middle, the outer hue on the arms.
    let warm = mix(bd.color3.rgb, bd.color0.rgb * 1.3, smoothstep(0.05, 0.9, r));
    col = col + warm * glow * 1.25 * bright;

    // Knots where stars are being born: small, bright, on the arms.
    let kn = bd_fbm(q * 9.0 + vec2<f32>(3.1, 7.7), 2);
    let knots = smoothstep(0.62, 0.8, kn) * arms * disc * smoothstep(0.12, 0.4, r);
    col = col + mix(bd.color1.rgb, vec3<f32>(1.0, 0.55, 0.8), step(0.5, bd_hash(floor(q * 9.0)))) * knots * 1.2 * bright;

    // The core.
    col = col + bd.color3.rgb * (exp(-r * r * 140.0) * 0.9 + exp(-r * 9.0) * 0.3) * bright;

    // Stars, in three depths, a few bright with a cross.
    let density = select(1.0, bd.params0.x, bd.params0.x > 0.0);
    for (var i = 0; i < 3; i = i + 1) {
        let fi = f32(i);
        let cell = 18.0 + fi * 24.0;
        let g = px / cell + vec2<f32>(fi * 13.1 + t * (2.0 + fi), fi * 7.7);
        let id = floor(g);
        let f = fract(g) - vec2<f32>(0.5);
        let h = bd_hash(id + vec2<f32>(fi * 31.0));
        if (h < 0.2 * density) {
            let off = vec2<f32>(bd_hash(id + 1.7), bd_hash(id + 4.3)) - vec2<f32>(0.5);
            let d = (f - off * 0.7) * cell;
            let l = length(d);
            let twinkle = 0.6 + 0.4 * sin(bd.time * (0.7 + h * 5.0) + h * 40.0);
            let size = 0.6 + fi * 0.5;
            var s = smoothstep(size + 0.8, 0.0, l);
            if (h < 0.012 * density) {
                // A bright one: a soft cross.
                s = s * 2.0 + (smoothstep(1.2, 0.0, abs(d.x)) + smoothstep(1.2, 0.0, abs(d.y))) * exp(-l * 0.25) * 0.5;
            }
            let tint = mix(bd.color3.rgb, mix(bd.color1.rgb, vec3<f32>(1.0), 0.6), bd_hash(id + 9.1));
            col = col + tint * s * twinkle * (0.45 + fi * 0.25);
        }
    }
    return vec4<f32>(col, 1.0);
}
