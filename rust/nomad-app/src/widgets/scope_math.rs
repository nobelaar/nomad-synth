//! Oscilloscope math of `nomad/gui/scope_view.py`: the trigger search and the
//! min/max decimation into drawing points (`y` computed in f32, as numpy does).

/// Index of the last rising zero crossing (`buf[i-1] < 0 <= buf[i]`), 0 if none.
/// NaN counts as 0; `-0.0` counts as negative, like `np.signbit`.
pub fn find_trigger(buf: &[f32]) -> usize {
    let neg = |x: f32| !x.is_nan() && x.is_sign_negative();
    (1..buf.len())
        .rev()
        .find(|&i| neg(buf[i - 1]) && !neg(buf[i]))
        .unwrap_or(0)
}

/// `nan_to_num` for f32: NaN -> 0, infinities -> the largest finite values.
fn clean(x: f32) -> f32 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(f32::MIN, f32::MAX)
    }
}

/// Drawing points (x, y) for `seg` in a `width` x `height` box: at most
/// `2*width` points (min/max per column), a flat line for empty input.
pub fn to_points(seg: &[f32], width: usize, height: usize, out: &mut Vec<(f32, f32)>) {
    out.clear();
    let (w, h) = (width as f32, height as f32);
    let half = (height as f64 / 2.0) as f32;
    let gain = (height as f64 / 2.0 - 2.0) as f32;
    if seg.is_empty() {
        out.extend([(0.0, h / 2.0), (w, h / 2.0)]);
        return;
    }
    let y = |s: f32| half - clean(s).clamp(-1.0, 1.0) * gain;
    let mut ys: Vec<f32> = Vec::with_capacity(seg.len().min(2 * width.max(1)));
    if width > 0 && seg.len() >= 2 * width {
        let k = seg.len() / width;
        for col in seg[..k * width].chunks(k) {
            let (lo, hi) = col
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &x| {
                    let x = clean(x);
                    (a.min(x), b.max(x))
                });
            ys.extend([y(lo), y(hi)]);
        }
    } else {
        ys.extend(seg.iter().map(|&s| y(s)));
    }
    if ys.len() == 1 {
        out.extend([(0.0, ys[0]), (w, ys[0])]);
        return;
    }
    // np.linspace(0, width, n): i * step, the last one exactly `width`.
    let n = ys.len();
    let step = width as f64 / (n - 1) as f64;
    out.extend(ys.iter().enumerate().map(|(i, &yv)| {
        let x = if i == n - 1 {
            width as f64
        } else {
            i as f64 * step
        };
        (x as f32, yv)
    }));
}
