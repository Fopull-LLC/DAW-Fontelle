// Lo-fi Rain: the window by the desk, late. A city at dusk beyond the
// glass — lit windows, a low moon, the street glowing at the bottom — and
// rain on the glass itself: mist, drops resting where they landed, and now
// and then one that runs down and leaves a clear trail behind it.
//
// Ty: *"the lofi rain one does not really resemble what it's describing it
// looks kind of just like colored dots flashing on a blurry screen. i think
// we could make a much prettier lofi rain effect inspired by the lofi girl
// study playlist aesthetic."* So the city is drawn, not blurred into dots.
//
// color0  lamplight and lit windows   color1  the dusk glow
// color2  the night sky               color3  the moon and the rain's glints

// Uv across the window; s keeps distances square (y is 0-1, x runs wider).
fn lofi_city(uv: vec2<f32>, t: f32) -> vec3<f32> {
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let s = vec2<f32>(uv.x * aspect, uv.y);

    // The sky: night at the top, the city's glow at the horizon.
    let up = smoothstep(0.78, 0.0, uv.y);
    var col = mix(bd.color1.rgb * 0.75 + bd.color0.rgb * 0.14, bd.color2.rgb * 1.15, up);
    col = mix(col, bd.color2.rgb * 0.70, smoothstep(0.35, 0.0, uv.y) * 0.6);
    // Slow clouds catching the glow from below.
    let cloud = bd_fbm(vec2<f32>(s.x * 2.2 + t * 0.010, s.y * 6.0), 4);
    col = col + bd.color1.rgb * smoothstep(0.45, 0.85, cloud) * 0.10 * smoothstep(0.05, 0.5, uv.y);

    // A few stars where the sky is darkest.
    let sg = s * 140.0;
    let sid = floor(sg);
    let sh = bd_hash(sid + vec2<f32>(2.0, 9.0));
    if (sh > 0.992 && uv.y < 0.42) {
        let d = length(fract(sg) - vec2<f32>(0.5));
        let twinkle = 0.6 + 0.4 * sin(t * (0.8 + sh * 2.0) + sh * 50.0);
        col = col + bd.color3.rgb * smoothstep(0.16, 0.0, d) * smoothstep(0.42, 0.05, uv.y) * 0.55 * twinkle;
    }

    // The moon, low on the right, with its halo.
    let mc = vec2<f32>(aspect * 0.80, 0.22);
    let md = length(s - mc);
    let moon = mix(bd.color3.rgb, vec3<f32>(1.0, 0.93, 0.85), 0.45);
    if (md < 0.045) {
        let mshade = 0.82 + 0.18 * bd_fbm((s - mc) * 40.0, 3);
        col = mix(col, moon * 0.95 * mshade, smoothstep(0.042, 0.038, md));
    }
    col = col + moon * exp(-md * 10.0) * 0.20;

    // Far towers: hazy, few windows.
    let fw = 0.045;
    let fx = floor(s.x / fw);
    let fh = 0.50 + bd_hash(vec2<f32>(fx, 3.0)) * 0.16 + step(0.86, bd_hash(vec2<f32>(fx, 8.0))) * 0.10;
    let far = smoothstep(fh, fh + 0.002, uv.y);
    let far_ink = mix(col, bd.color2.rgb * 0.95 + bd.color1.rgb * 0.08, 0.75);
    let fcell = floor(vec2<f32>(s.x / (fw / 4.0), uv.y / 0.016));
    let flit = step(0.88, bd_hash(fcell + vec2<f32>(5.0, 1.0)));
    let fin = step(0.25, fract(s.x / (fw / 4.0))) * step(0.35, fract(uv.y / 0.016));
    col = mix(col, far_ink + bd.color0.rgb * flit * fin * 0.25, far);

    // Near buildings: dark, with the lit windows of people still up.
    let nw = 0.085;
    let nx = floor(s.x / nw);
    let nh = 0.62 + bd_hash(vec2<f32>(nx, 11.0)) * 0.14;
    let near = smoothstep(nh, nh + 0.002, uv.y);
    let ncols = 4.0 + floor(bd_hash(vec2<f32>(nx, 13.0)) * 4.0);
    let nrow = 0.020 + bd_hash(vec2<f32>(nx, 17.0)) * 0.010;
    let ncell = floor(vec2<f32>(s.x / (nw / ncols), uv.y / nrow));
    // A window changes its mind every minute or so, each on its own clock.
    let nphase = floor(t / 47.0 + bd_hash(ncell) * 9.0);
    let nlit = step(0.80, bd_hash(ncell + vec2<f32>(nphase, 4.0)));
    let nin = step(0.32, fract(s.x / (nw / ncols))) * step(0.36, fract(uv.y / nrow));
    let warm = mix(bd.color0.rgb, bd.color1.rgb, step(0.8, bd_hash(ncell + 7.0)));
    let low = smoothstep(1.0, nh, uv.y);
    let near_ink = bd.color2.rgb * (0.30 + 0.18 * low) + warm * nlit * nin * (0.55 + 0.55 * low);
    col = mix(col, near_ink, near);

    // The street's glow, and its lamps out of focus.
    col = col + bd.color0.rgb * smoothstep(0.80, 1.05, uv.y) * 0.14;
    for (var i = 0; i < 7; i = i + 1) {
        let fi = f32(i);
        let lx = fract(bd_hash(vec2<f32>(fi, 1.0)) + t * 0.004 * (0.5 + bd_hash(vec2<f32>(fi, 2.0))));
        let lp = vec2<f32>(lx * aspect, 0.90 + bd_hash(vec2<f32>(fi, 3.0)) * 0.07);
        let r = 0.022 + bd_hash(vec2<f32>(fi, 4.0)) * 0.02;
        let d = length(s - lp);
        let lc = mix(bd.color0.rgb, bd.color1.rgb, step(0.65, bd_hash(vec2<f32>(fi, 5.0))));
        col = col + lc * smoothstep(r, r * 0.6, d) * 0.22;
    }
    return col;
}

// One drop on the glass, `v` from its middle and `r` its size, in points:
// x how much of it is here, y its rim, z the glint up on its shoulder.
fn lofi_drop(v: vec2<f32>, r: f32) -> vec3<f32> {
    let d = length(v);
    let m = smoothstep(r, r - 1.0, d);
    let rim = smoothstep(r - 2.2, r - 0.6, d) * m;
    let spot = smoothstep(r * 0.30, r * 0.10, length(v + vec2<f32>(r * 0.32, r * 0.38)));
    return vec3<f32>(m, rim, spot * m);
}

fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let t = bd.time;
    // Points across the window, so drops are the same size on any screen.
    let p = uv * bd.window / max(bd.scale, 0.001);

    // What the drops do: `lens` where there is one, the way it bends the
    // city behind it (a drop is a small lens, so it shows it upside down),
    // its darker rim and its glint.
    var lens = 0.0;
    var bend = vec2<f32>(0.0);
    var rim = 0.0;
    var glint = 0.0;

    // Drops resting where they landed. Each sits a while, and dries.
    let dcell = 34.0;
    let dg = p / dcell;
    let did = floor(dg);
    let dh = bd_hash(did + vec2<f32>(4.0, 8.0));
    if (dh < 0.40) {
        let life = fract(t * 0.025 + dh * 7.0);
        let c = vec2<f32>(bd_hash(did + 0.7), bd_hash(did + 3.3)) * 0.6 + vec2<f32>(0.2);
        let v = (fract(dg) - c) * dcell * vec2<f32>(1.0, 1.12);
        let r = (3.0 + 6.0 * bd_hash(did + 6.1) * bd_hash(did + 1.9)) * smoothstep(1.0, 0.8, life);
        let k = lofi_drop(v, r);
        lens = k.x;
        rim = k.y;
        glint = k.z;
        bend = v / max(r, 0.5) * k.x * vec2<f32>(0.010, 0.030);
    }

    // A few drops running down, each leaving clear glass behind it.
    var clear = 0.0;
    let lane = 72.0;
    let lx = floor(p.x / lane);
    let lh = bd_hash(vec2<f32>(lx, 21.0));
    if (lh < 0.55) {
        let fall = bd.window.y + 200.0;
        let speed = 22.0 + lh * 40.0;
        // In fits and starts, as rain on a window runs.
        let tt = t * speed + sin(t * 0.9 + lh * 9.0) * 18.0 + bd_hash(vec2<f32>(lx, 4.0)) * fall;
        let y_head = (tt % fall) - 100.0;
        let wob = sin(p.y * 0.030 + lh * 20.0) * 6.0 + sin(p.y * 0.10 + lh * 3.0) * 1.5;
        let cx = (lx + 0.3 + 0.4 * bd_hash(vec2<f32>(lx, 6.0))) * lane + wob;
        let hv = vec2<f32>(p.x - cx, (p.y - y_head) * 0.8);
        let k = lofi_drop(hv, 9.0);
        if (k.x > lens) {
            lens = k.x;
            rim = k.y;
            glint = k.z;
            bend = hv / 9.0 * k.x * vec2<f32>(0.014, 0.040);
        }
        // The trail: clear where it ran, misting over again behind it.
        let behind = y_head - p.y;
        let width = 4.5 * smoothstep(380.0, 0.0, behind);
        clear = smoothstep(width, width * 0.3, abs(p.x - cx)) * step(0.0, behind) * smoothstep(460.0, 30.0, behind);
        // And the beads it leaves on the way.
        let row = floor(p.y / 22.0 + lh * 3.0);
        let bv = vec2<f32>(p.x - cx, (fract(p.y / 22.0 + lh * 3.0) - 0.5) * 22.0);
        let br = (2.0 + 1.5 * bd_hash(vec2<f32>(lx, row + 0.5))) * step(0.5, bd_hash(vec2<f32>(lx, row)))
            * step(12.0, behind) * smoothstep(420.0, 80.0, behind);
        let b = lofi_drop(bv, br);
        if (b.x > lens) {
            lens = b.x;
            rim = b.y;
            glint = b.z;
            bend = bv / max(br, 0.5) * b.x * vec2<f32>(0.006, 0.016);
        }
    }

    var col = lofi_city(uv + bend, t);
    // Mist on the glass softens the city and lifts its blacks — except
    // where a drop has run, and inside the drops, which see clearly.
    let mist = 0.30 * (1.0 - clear) * (1.0 - lens);
    let haze = mix(bd.color2.rgb * 1.25, bd.color1.rgb * 0.40 + bd.color2.rgb, smoothstep(0.2, 1.0, uv.y));
    col = mix(col, haze + col * 0.40, mist);
    // A drop gathers light from all round it, so over a dark wall it is
    // a little lighter than the wall, never a hole in it.
    col = mix(col, col * 1.12 + haze * 0.30, lens) * (1.0 - rim * 0.25);
    col = col + mix(bd.color3.rgb, vec3<f32>(1.0), 0.6) * glint * 0.85;

    // The desk lamp behind you, reflected low in the glass.
    let aspect = bd.window.x / max(bd.window.y, 1.0);
    let lamp = length(vec2<f32>((uv.x - 0.06) * aspect, uv.y - 1.02));
    col = col + bd.color0.rgb * exp(-lamp * 3.2) * 0.20;
    return vec4<f32>(col, 1.0);
}
