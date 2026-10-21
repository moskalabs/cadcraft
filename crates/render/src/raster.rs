//! CPU rasteriser: draws a display list with tiny-skia (PNG export, plot preview, tests).

use cadcraft_color::Rgb;
use cadcraft_geom::{Bounds2, Vec2};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::{DisplayList, Kind};

/// A 2D view: world point at the image centre and pixels per drawing unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub center: Vec2,
    pub scale: f64,
    pub width: u32,
    pub height: u32,
}

impl View {
    pub fn to_screen(&self, p: Vec2) -> (f32, f32) {
        let x = (p.x - self.center.x) * self.scale + f64::from(self.width) / 2.0;
        let y = f64::from(self.height) / 2.0 - (p.y - self.center.y) * self.scale;
        (x as f32, y as f32)
    }
    /// Fit `b` with a margin fraction.
    pub fn fit(b: &Bounds2, width: u32, height: u32, margin: f64) -> View {
        if b.is_empty() {
            return View { center: Vec2::ZERO, scale: 1.0, width, height };
        }
        let w = b.width().max(1e-9);
        let h = b.height().max(1e-9);
        let s = (f64::from(width) / w).min(f64::from(height) / h) * (1.0 - margin);
        View { center: b.center(), scale: if s.is_finite() && s > 0.0 { s } else { 1.0 }, width, height }
    }
    /// The view of `window` (drawing units), filling the image: `width` × `height` pixels when both
    /// are given; otherwise the side given (default 1600 wide) with the other matching the window's
    /// shape, each side 16..=8192.
    pub fn of_window(window: &Bounds2, width: Option<u32>, height: Option<u32>) -> View {
        let aspect = if window.width() > 1e-12 && window.height() > 1e-12 { window.width() / window.height() } else { 1.0 };
        let side = |v: f64| v.round().clamp(16.0, 8192.0) as u32;
        let (w, h) = match (width, height) {
            (Some(w), Some(h)) => (side(f64::from(w)), side(f64::from(h))),
            (None, Some(h)) => (side(f64::from(h) * aspect), side(f64::from(h))),
            (w, None) => {
                let w = f64::from(w.unwrap_or(1600));
                (side(w), side(w / aspect))
            }
        };
        View::fit(window, w, h, 0.0)
    }
    pub fn world_bounds(&self) -> Bounds2 {
        let hw = f64::from(self.width) / 2.0 / self.scale;
        let hh = f64::from(self.height) / 2.0 / self.scale;
        Bounds2::new(self.center - Vec2::new(hw, hh), self.center + Vec2::new(hw, hh))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RasterOptions {
    pub background: Rgb,
    /// Line width in pixels for zero-lineweight lines.
    pub hairline: f32,
    /// Pixels per mm for lineweights (0 = ignore lineweights).
    pub px_per_mm: f32,
    pub antialias: bool,
}

impl Default for RasterOptions {
    fn default() -> Self {
        RasterOptions { background: Rgb(33, 40, 48), hairline: 1.0, px_per_mm: 0.0, antialias: true }
    }
}

/// Render to a pixmap. Returns `None` for zero or absurd sizes.
pub fn render(list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Pixmap> {
    if view.width == 0 || view.height == 0 || view.width > 16384 || view.height > 16384 {
        return None;
    }
    let mut pm = Pixmap::new(view.width, view.height)?;
    pm.fill(tiny_skia::Color::from_rgba8(o.background.0, o.background.1, o.background.2, 255));
    let vis = view.world_bounds();
    // 같은 붓으로 **잇달아** 오는 선들을 한 path 에 모아 한 번에 긋는다.
    //
    // 전엔 프리미티브마다 `PathBuilder` 를 새로 만들고 `stroke_path` 를 따로
    // 불렀다. 큰 도면은 그게 한 프레임에 1만 번을 넘고, 호출마다 윤곽선
    // path 를 새로 만드는 비용이 붙는다 — 점을 옮기는 일보다 그 간접비가
    // 크다.
    //
    // **잇달아 오는 것만** 묶는다. 멀리 있는 같은 색을 끌어다 붙이면 그리는
    // 차례가 바뀌어서, 문·창의 가림이나 공간의 비치는 면이 엉뚱한 것을
    // 덮는다 (`opening.rs`·`zone.rs` 가 그 차례에 기대고 있다).
    //
    // **불투명한 것만** 묶는다. 반투명한 선 둘이 겹칠 때, 따로 그으면 두 번
    // 섞이고 한 path 로 그으면 한 번 섞인다 — 그림이 달라진다.
    let mut brush: Option<Brush> = None;
    let mut pb = PathBuilder::new();
    for p in &list.prims {
        let c = p.display_rgb(o.background);
        let mut paint = Paint::default();
        // A transparent object blends with what is drawn under it.
        paint.set_color_rgba8(c.0, c.1, c.2, p.alpha());
        paint.anti_alias = o.antialias;
        let pts = list.points(p);
        match p.kind {
            Kind::Polyline => {
                if !visible(pts, &vis) {
                    continue;
                }
                let w = if o.px_per_mm > 0.0 && p.lw > 0.0 { (p.lw * o.px_per_mm).max(o.hairline) } else { o.hairline };
                let want = Brush { rgb: c, alpha: p.alpha(), width: w };
                // 붓이 바뀌면 쓰던 것을 먼저 긋는다
                if brush != Some(want) {
                    flush(&mut pm, &mut pb, &mut brush, o.antialias);
                    brush = Some(want);
                }
                for (i, q) in pts.iter().enumerate() {
                    let (x, y) = view.to_screen(*q);
                    if i == 0 {
                        pb.move_to(x, y);
                    } else {
                        pb.line_to(x, y);
                    }
                }
                // 반투명은 모으지 않는다 — 겹치면 섞이는 횟수가 달라진다
                if want.alpha != 255 {
                    flush(&mut pm, &mut pb, &mut brush, o.antialias);
                }
                continue;
            }
            _ => flush(&mut pm, &mut pb, &mut brush, o.antialias),
        }
        match p.kind {
            // 위에서 `continue` 했다
            Kind::Polyline => {}
            Kind::Tris | Kind::Mask => {
                if p.kind == Kind::Mask {
                    paint.set_color_rgba8(o.background.0, o.background.1, o.background.2, 255);
                }
                let mut pb = PathBuilder::new();
                for t in pts.chunks(3) {
                    if let [a, b, c] = t {
                        let (ax, ay) = view.to_screen(*a);
                        let (bx, by) = view.to_screen(*b);
                        let (cx, cy) = view.to_screen(*c);
                        pb.move_to(ax, ay);
                        pb.line_to(bx, by);
                        pb.line_to(cx, cy);
                        pb.close();
                    }
                }
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                }
            }
            Kind::Point => {
                if let Some(q) = pts.first() {
                    let (x, y) = view.to_screen(*q);
                    // Filled as a path: tiny-skia's anti-aliased `fill_rect` trips a debug
                    // assertion on rects under two pixels wide whose sides both fall mid-pixel.
                    if let Some(r) = tiny_skia::Rect::from_xywh(x - 0.5, y - 0.5, 1.5, 1.5) {
                        pm.fill_path(&PathBuilder::from_rect(r), &paint, FillRule::Winding, Transform::identity(), None);
                    }
                }
            }
            Kind::Infinite { ray } => {
                let (Some(base), Some(dir)) = (pts.first(), pts.get(1)) else { continue };
                let far = (vis.width() + vis.height()) * 2.0 + base.dist(vis.center());
                let a = if ray { *base } else { *base - *dir * far };
                let b = *base + *dir * far;
                let mut pb = PathBuilder::new();
                let (ax, ay) = view.to_screen(a);
                let (bx, by) = view.to_screen(b);
                pb.move_to(ax, ay);
                pb.line_to(bx, by);
                if let Some(path) = pb.finish() {
                    pm.stroke_path(&path, &paint, &Stroke { width: o.hairline, ..Default::default() }, Transform::identity(), None);
                }
            }
        }
    }
    flush(&mut pm, &mut pb, &mut brush, o.antialias);
    Some(pm)
}

/// 이 선이 화면에 걸치나 — **걸치는 걸 알자마자 그만둔다.**
///
/// 폴리라인이 화면에 걸치는지는 제 경계상자가 화면과 겹치는지로 본다. 전엔
/// `Bounds2::from_points` 로 **점을 끝까지 훑어** 상자를 만들고 나서 봤다.
/// 보이는 선(흔한 쪽)은 몇 점만 봐도 이미 겹치는데도 그랬다 — 큰 도면은
/// 한 프레임에 61만 점을 그렇게 한 번 더 돌았다 (path 를 만들 때 또 돈다).
///
/// 상자를 키워 가다 겹치는 순간 멈춘다. 안 보이는 선은 어차피 끝까지 봐야
/// 하지만, 그건 그리기를 통째로 건너뛰는 값이라 싸다.
fn visible(pts: &[Vec2], vis: &Bounds2) -> bool {
    let mut bb = Bounds2::EMPTY;
    for q in pts {
        bb.add(*q);
        if bb.intersects(vis) {
            return true;
        }
    }
    false
}

/// The paint a run of polylines shares.
#[derive(Clone, Copy, PartialEq)]
struct Brush {
    rgb: Rgb,
    alpha: u8,
    width: f32,
}

/// Stroke what has been gathered, and start a fresh path.
fn flush(pm: &mut Pixmap, pb: &mut PathBuilder, brush: &mut Option<Brush>, antialias: bool) {
    let Some(b) = brush.take() else { return };
    let built = std::mem::replace(pb, PathBuilder::new());
    let Some(path) = built.finish() else { return };
    let mut paint = Paint::default();
    paint.set_color_rgba8(b.rgb.0, b.rgb.1, b.rgb.2, b.alpha);
    paint.anti_alias = antialias;
    let stroke = Stroke { width: b.width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// Render and encode as PNG.
pub fn render_png(list: &DisplayList, view: &View, o: &RasterOptions) -> Option<Vec<u8>> {
    render(list, view, o)?.encode_png().ok()
}
