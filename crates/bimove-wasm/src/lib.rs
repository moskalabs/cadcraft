//! Bimove 의 WASM 바인딩.
//!
//! CADCraft 의 crate(`doc`/`io`/`render`)를 브라우저에서 쓰기 위한 얇은 껍데기다.
//! **CADCraft 쪽 코드는 건드리지 않는다** — 업스트림을 계속 당겨올 것이고,
//! 그들 파일을 고치면 머지가 깨지는 데다 Apache-2.0 §4(b) 변경 표시 의무까지
//! 생긴다. 우리 것은 전부 이 crate 안에만 둔다.
//!
//! JS 로 넘기는 모양은 **평평한 숫자 배열**이다. 객체 수만 개를 wasm-bindgen
//! 으로 하나씩 넘기면 경계를 넘는 비용이 그리는 비용보다 커진다. 도면 하나가
//! 수만 개 선분이라 이게 전부다.

pub mod bimove;
pub mod wall;

use cadcraft_doc::Space;
use cadcraft_engine::Session;
use cadcraft_geom::Vec2;
use cadcraft_render::{DisplayList, Kind, Options};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// wasm 이 아닌 곳(테스트·네이티브 검증)에서도 같은 코드를 쓰기 위한 더미.
#[cfg(not(target_arch = "wasm32"))]
macro_rules! wasm_bindgen {
    ($($t:tt)*) => {};
}

// ── 한 프리미티브당 정수 몇 개를 싣는가 ──
//
// [kind, rgb, start, len, lw_um, flags]
// 이 숫자가 바뀌면 TS 쪽 읽는 루프도 같이 고쳐야 한다. 그래서 상수로 내보낸다.
const PRIM_STRIDE: usize = 6;

const KIND_POLYLINE: i32 = 0;
const KIND_TRIS: i32 = 1;
const KIND_MASK: i32 = 2;
const KIND_POINT: i32 = 3;
const KIND_LINE_INF: i32 = 4;
const KIND_RAY: i32 = 5;

fn kind_code(k: Kind) -> i32 {
    match k {
        Kind::Polyline => KIND_POLYLINE,
        Kind::Tris => KIND_TRIS,
        Kind::Mask => KIND_MASK,
        Kind::Point => KIND_POINT,
        Kind::Infinite { ray: false } => KIND_LINE_INF,
        Kind::Infinite { ray: true } => KIND_RAY,
    }
}

/// 그릴 거리 한 묶음. `verts`/`tris` 는 x,y 가 번갈아 들어간 평평한 배열이다.
pub struct Frame {
    verts: Vec<f64>,
    tris: Vec<f64>,
    pub prims: Vec<i32>,
    bounds: [f64; 4],
    expanded: usize,
}

impl Frame {
    fn from_list(dl: &DisplayList) -> Frame {
        let mut verts = Vec::with_capacity(dl.verts.len() * 2);
        for v in &dl.verts {
            verts.push(v.x);
            verts.push(v.y);
        }
        let mut tris = Vec::with_capacity(dl.tris.len() * 2);
        for v in &dl.tris {
            tris.push(v.x);
            tris.push(v.y);
        }
        let mut prims = Vec::with_capacity(dl.prims.len() * PRIM_STRIDE);
        for p in &dl.prims {
            prims.push(kind_code(p.kind));
            prims.push(p.color.to_u32() as i32);
            prims.push(p.start as i32);
            prims.push(p.len as i32);
            // 선 굵기는 mm 인데 정수로 싣는다 — µm 단위로 올려서 소수점을 없앤다.
            prims.push((f64::from(p.lw) * 1000.0).round() as i32);
            // 비트 0: aci7 (색 7번 — 배경에 따라 흑/백이 뒤집히는 색)
            prims.push(i32::from(p.aci7));
        }
        Frame {
            verts,
            tris,
            prims,
            bounds: [dl.bounds.min.x, dl.bounds.min.y, dl.bounds.max.x, dl.bounds.max.y],
            expanded: dl.expanded,
        }
    }
}

/// WASM 안에서 래스터라이즈한 한 장. `rgba` 는 캔버스 `ImageData` 에 바로 넣는다.
pub struct Raster {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
}

impl Doc {
    /// 도면을 **픽셀로** 그려서 돌려준다.
    ///
    /// DisplayList 를 JS 로 넘겨 Canvas2D 로 그리는 길도 있지만, 큰 도면에서는
    /// 그쪽이 막힌다 — 프리미티브가 10만 개면 Path2D 를 아무리 묶어도 한 프레임이
    /// 수 초다. 여기서는 CADCraft 자신이 쓰는 CPU 래스터라이저(tiny-skia)로 그린다.
    ///
    /// `cx`,`cy` 는 화면 **가운데**에 올 도면 좌표, `scale` 은 도면 단위당 픽셀.
    #[allow(clippy::too_many_arguments)]
    /// 계측용 — 덜어내기가 점을 얼마나 줄였나. `(전, 후)`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn decimate_report(&self, layout: &str, tol: f64) -> (usize, usize) {
        let space = Self::space_of(layout);
        let opts = Options { tolerance: tol, ..Options::default() };
        let mut dl = cadcraft_render::build(self.drawing(), &space, &opts);
        decimate(&mut dl, tol)
    }

    /// 계측용 — 그릴 거리를 만드는 비용만 잰다.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn build_for_bench(&self, layout: &str, tolerance: f64) -> Frame {
        self.build(layout, tolerance)
    }

    /// 계측용 창구. 네이티브 예제(`examples/bimove_bench.rs`)가 쓴다 —
    /// wasm 바깥에서 재야 브라우저를 띄우지 않고 프레임 시간을 볼 수 있다.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn raster_for_bench(
        &mut self, layout: &str, cx: f64, cy: f64, scale: f64, w: u32, h: u32, bg: u32,
        draft: bool,
    ) -> Option<Raster> {
        self.raster(layout, cx, cy, scale, w, h, bg, draft)
    }

    fn raster(
        &mut self, layout: &str, cx: f64, cy: f64, scale: f64, w: u32, h: u32, bg: u32,
        draft: bool,
    ) -> Option<Raster> {
        let bg_rgb = cadcraft_color::Rgb((bg >> 16) as u8, (bg >> 8) as u8, bg as u8);
        // 곡선을 선분으로 쪼갤 오차는 "화면 반 픽셀". 줌이 바뀌면 같이 바뀐다.
        let tol = 0.5 / scale.max(1e-12);
        // 배경색도 키다 — 색 보정이 배경에 달려 있어서, 배경이 바뀌면 같은
        // 목록을 쓰면 안 된다.
        let rev = self.revision();
        let reusable = self.cache.as_ref().is_some_and(|(l, t, b, r, _)| {
            l == layout
                && *b == bg
                && *r == rev
                // `draft` 면 tolerance 가 얼마나 어긋나든 쓰던 걸 쓴다.
                //
                // 휠을 굴리는 동안은 한 옥타브마다 목록을 새로 만든다. 60만
                // 선분짜리에서 그게 **프레임당 150ms** 다 (실측). 그동안 JS 가
                // 막혀 있어서 브라우저는 직전 그림을 계속 보여준다 — 한이 본
                // "줌할 때 텍스트 잔상" 이 그거다.
                //
                // 거친 목록으로 한 프레임 그리는 건 손해가 거의 없다. 우리
                // 도면은 들여올 때 이미 선분으로 평평해져서 tolerance 가
                // 바꾸는 건 글자 외곽선뿐이다. 손을 떼면 호출부가 draft 를
                // 끄고 한 번 더 그려서 제대로 맞춘다.
                && (draft || (*t / TOL_SLACK <= tol && tol <= *t * TOL_SLACK))
        });
        if !reusable {
            // **만들기 전에 옛것을 놓는다.** 그냥 덮어쓰면 두 목록이 잠깐
            // 동시에 산다 — 60만 선분이면 그만큼이 한 번 더 필요하다.
            self.cache = None;
            let space = Self::space_of(layout);
            let opts = Options { tolerance: tol, ..Options::default() };
            let mut dl = cadcraft_render::build(self.drawing(), &space, &opts);
            make_readable(&mut dl, bg_rgb);
            // 그릴 때 쓰는 tolerance 와 **같은 값**으로 덜어낸다. 그래야
            // 보이는 차이가 없다는 보장이 선다 (decimate 주석 참고).
            decimate(&mut dl, tol);
            self.cache = Some((layout.to_string(), tol, bg, rev, dl));
        }
        let dl = &self.cache.as_ref()?.4;
        let view = cadcraft_render::raster::View {
            center: cadcraft_geom::Vec2 { x: cx, y: cy },
            scale,
            width: w,
            height: h,
        };
        let ro = cadcraft_render::raster::RasterOptions { background: bg_rgb, ..Default::default() };
        let mut pm = cadcraft_render::raster::render(dl, &view, &ro)?;

        // tiny-skia 는 **알파 곱해진(premultiplied)** 픽셀을 준다. 캔버스
        // ImageData 는 곱하지 않은 값을 원한다 — 그냥 넘기면 반투명한 것이
        // 어둡게 나온다. 여기서 되돌린다.
        //
        // **제자리에서** 되돌린다. 새 Vec 에 담으면 1874×1000 한 장에 7.5MB 를
        // 프레임마다 더 잡는다 — 그리는 비용에 비하면 작지만, 줌에서 목록을
        // 다시 만드는 순간과 겹치면 가장 아쉬운 자리다.
        demultiply_in_place(pm.data_mut());
        Some(Raster { rgba: pm.take(), w, h })
    }
}

// ── 화면에서 구분되지 않는 점 덜어내기 ──
//
// 전체 보기에서는 폴리라인 대부분이 1픽셀도 안 되는 디테일이다. 그걸 다 그리는
// 게 프레임 시간의 거의 전부다 — 래스터라이저는 선분마다 화면 좌표로 옮기고
// stroke 하므로 **비용이 선분 개수에 붙는다.** (해상도를 반으로 낮춰도 242→
// 186ms 밖에 안 줄었다. 픽셀은 1/4 인데. 그래서 덜 그리는 것 말고 길이 없다.)
//
// 엔진의 `tolerance` 는 **곡선을 선분으로 쪼갤 때**만 쓰인다. 이미 선분인
// 폴리라인은 줌을 아무리 빼도 점 수가 그대로다. 우리 도면은 들여올 때 전부
// 선분으로 평평해져서 정확히 그 경우에 걸린다.
//
// 그래서 만들어진 그릴 거리에서 **화면 반 픽셀 안쪽인 점을 덜어낸다.**
// Douglas–Peucker 는 "간략화한 선이 원래 선에서 ε 안에 있다" 를 보장하므로,
// ε 를 그릴 때 쓰는 tolerance 와 같게 두면 **눈에 보이는 차이가 없다.**
// 줌을 당기면 tolerance 가 작아지고 다시 만들면서 디테일이 돌아온다.
//
// 도면 자체(Drawing)는 손대지 않는다. 고르기·스냅·편집은 전부 엔티티를 보므로
// 정확도가 그대로다 — 싸지는 건 픽셀뿐이다.

/// 점에서 선분 `a`–`b` 까지의 거리. `a == b` 면 점까지의 거리.
fn perp_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::MIN_POSITIVE {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    let (qx, qy) = (a.x + t * dx, a.y + t * dy);
    ((p.x - qx).powi(2) + (p.y - qy).powi(2)).sqrt()
}

/// Douglas–Peucker. `keep[i]` 가 남길 점.
///
/// 재귀가 아니라 **제 스택**으로 돈다 — 한 폴리라인이 수천 점일 수 있어서,
/// 최악의 경우(계단 모양) 재귀 깊이가 그만큼 간다.
fn douglas_peucker(pts: &[Vec2], eps: f64, keep: &mut Vec<bool>) {
    keep.clear();
    keep.resize(pts.len(), false);
    if pts.len() < 2 {
        keep.iter_mut().for_each(|k| *k = true);
        return;
    }
    let last = pts.len() - 1;
    keep[0] = true;
    keep[last] = true;
    let mut stack = vec![(0usize, last)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (pts[a], pts[b]);
        let mut best = 0.0;
        let mut bi = a;
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = perp_dist(*p, pa, pb);
            if d > best {
                best = d;
                bi = i;
            }
        }
        if best > eps && bi > a {
            keep[bi] = true;
            stack.push((a, bi));
            stack.push((bi, b));
        }
    }
}

/// 그릴 거리의 폴리라인에서 ε 안쪽 점을 덜어낸다. `(전, 후)` 점 수.
///
/// **프리미티브를 지우지는 않는다** — 점만 줄인다. 글자(`texts[].prim`)와
/// 레이어(`prim_layers`)가 프리미티브 번호로 서로를 가리키고 있어서, 하나라도
/// 빼면 그 참조가 전부 어긋난다.
fn decimate(dl: &mut DisplayList, eps: f64) -> (usize, usize) {
    let before = dl.verts.len();
    if !(eps.is_finite() && eps > 0.0) || dl.verts.is_empty() {
        return (before, before);
    }
    let mut out: Vec<cadcraft_geom::Vec2> = Vec::with_capacity(dl.verts.len());
    let mut keep: Vec<bool> = Vec::new();
    for p in &mut dl.prims {
        // Tris/Mask 는 `tris` 를 가리킨다 — 여기서 건드릴 것이 아니다.
        if matches!(p.kind, Kind::Tris | Kind::Mask) {
            continue;
        }
        let (s, l) = (p.start as usize, p.len as usize);
        let Some(pts) = dl.verts.get(s..s + l) else { continue };
        let start = out.len() as u32;
        if l <= 2 {
            out.extend_from_slice(pts);
        } else {
            douglas_peucker(pts, eps, &mut keep);
            out.extend(pts.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p));
        }
        p.start = start;
        p.len = out.len() as u32 - start;
    }
    let after = out.len();
    dl.verts = out;
    (before, after)
}

/// 알파 곱해진 RGBA 바이트를 **제자리에서** 곱하지 않은 값으로 되돌린다.
///
/// tiny-skia 의 `PremultipliedColorU8::demultiply()` 와 같은 값을 내야 한다 —
/// 반올림이 1 이라도 어긋나면 반투명한 해치가 미묘하게 다른 색이 된다.
/// 그래서 그 함수를 그대로 부른다.
fn demultiply_in_place(data: &mut [u8]) {
    use tiny_skia::PremultipliedColorU8;
    for px in data.chunks_exact_mut(4) {
        // 불투명(가장 흔한 경우)은 되돌릴 게 없다.
        if px[3] == 255 {
            continue;
        }
        let Some(p) = PremultipliedColorU8::from_rgba(px[0], px[1], px[2], px[3]) else {
            continue;
        };
        let c = p.demultiply();
        px[0] = c.red();
        px[1] = c.green();
        px[2] = c.blue();
        px[3] = c.alpha();
    }
}

// ── 흰 배경에서 안 보이는 색 ──
//
// CAD 색(ACI)은 오토캐드의 **검은 모형공간**을 전제로 고른 것이다. 노랑(2)과
// 하늘(4)이 대표적인데, 흰 배경에 그대로 올리면 글자가 거의 안 보인다.
// CADCraft 는 색 7번만 배경에 따라 흑백을 뒤집고(display_rgb) 나머지는 그대로
// 둔다 — 종이에 출력할 땐 플롯 스타일이 그 일을 하기 때문이다.
//
// Bimove 는 뷰어이지 플로터가 아니다. 흰 캔버스에서 도면을 **읽는** 게 목적이라
// 묻히는 색만 끌어내린다. **밝은 배경에서는 색상이 보존된다** — 전 성분을 같은
// 비율로 줄이므로 "노란 글씨" 는 어두운 겨자색, 하늘색은 청록으로 남는다.
// 레이어를 색으로 구분하던 사람이 길을 잃지 않는다.
//
// 어두운 배경에서는 채도가 빠진다. 진한 파랑은 성분을 키워도 밝아지지 않아서
// (루마에서 파랑의 몫이 0.07 뿐이다) 흰색을 섞는 수밖에 없다. 받아들인다 —
// Bimove 의 캔버스는 흰색이다.

/// 배경과 이만큼은 밝기 차가 나야 읽힌다 (luma 0~1).
///
/// 0.4 는 노랑·하늘이 여전히 연해서 치수가 잘 안 읽혔다. 0.55 면 흰 배경에서
/// 글씨가 또렷하면서도 색은 남는다 — 하늘(0,255,255)이 (0,164,164) 이 되고
/// 파랑(0,0,255)은 이미 충분히 어두워 손대지 않으므로 둘은 그대로 구분된다.
/// 더 올리면 색이 전부 검정 쪽으로 몰려 레이어 구분이 흐려진다.
const MIN_INK_CONTRAST: f64 = 0.55;

fn scale_rgb(c: cadcraft_color::Rgb, f: f64) -> cadcraft_color::Rgb {
    let q = |v: u8| (f64::from(v) * f).round().clamp(0.0, 255.0) as u8;
    cadcraft_color::Rgb(q(c.0), q(c.1), q(c.2))
}

/// `bg` 위에서 읽히도록 밝기만 옮긴 색.
fn readable_on(c: cadcraft_color::Rgb, bg: cadcraft_color::Rgb) -> cadcraft_color::Rgb {
    let (lb, lc) = (bg.luma(), c.luma());
    if lb >= 0.5 {
        // 밝은 배경 — 잉크를 어둡게. luma 가 성분의 선형 가중합이라 전부 같은
        // 비율로 줄이면 목표 밝기에 정확히 떨어지고 색상은 보존된다.
        let target = lb - MIN_INK_CONTRAST;
        if lc <= target {
            return c;
        }
        scale_rgb(c, (target / lc.max(1e-9)).clamp(0.0, 1.0))
    } else {
        // 어두운 배경 — 흰색 쪽으로 섞어 밝힌다 (줄이는 걸론 밝아지지 않는다).
        let target = lb + MIN_INK_CONTRAST;
        if lc >= target {
            return c;
        }
        let t = ((target - lc) / (1.0 - lc).max(1e-9)).clamp(0.0, 1.0);
        let up = |v: u8| (f64::from(v) + (255.0 - f64::from(v)) * t).round() as u8;
        cadcraft_color::Rgb(up(c.0), up(c.1), up(c.2))
    }
}

/// 배경에 묻히는 색을 끌어낸다. **DisplayList 를 제자리에서 고친다.**
fn make_readable(dl: &mut DisplayList, bg: cadcraft_color::Rgb) {
    for p in &mut dl.prims {
        // 색 7번은 display_rgb 가 이미 배경에 맞춰 흑백으로 뒤집는다. 그 결과를
        // 가지고 판단해야 "이미 검은 글씨" 를 또 손대지 않는다.
        let shown = cadcraft_color::display_rgb(p.color, p.aci7, bg);
        let fixed = readable_on(shown, bg);
        if fixed != shown {
            p.color = fixed;
            // 우리가 정한 색을 그대로 쓰게 한다 (다시 흑백으로 뒤집히지 않도록)
            p.aci7 = false;
        }
    }
}

/// 커서가 붙을 점 하나.
pub struct Snap {
    x: f64,
    y: f64,
    /// 어떤 스냅인지 — "Endpoint", "Midpoint", "Perpendicular" …
    name: &'static str,
    mode: u32,
}

impl Doc {
    fn space_of(layout: &str) -> Space {
        if layout.is_empty() {
            Space::Model
        } else {
            Space::Paper(layout.to_string())
        }
    }

    /// 한 점을 찍어 엔티티를 고른다. 없으면 None.
    ///
    /// `aperture` 는 **도면 단위**다 — 화면 픽셀이 아니라. 줌이 바뀌면 같은
    /// 화면 거리라도 도면 거리가 달라지므로, 부르는 쪽이 `픽셀 / scale` 로
    /// 바꿔 넘겨야 한다. 안 그러면 줌인했을 때 클릭이 안 먹는다.
    fn pick(&self, layout: &str, x: f64, y: f64, aperture: f64) -> Option<String> {
        let h = cadcraft_engine::select::pick(
            self.drawing(),
            &Self::space_of(layout),
            cadcraft_geom::Vec2 { x, y },
            aperture,
        )?;
        Some(format!("{h:?}"))
    }

    /// 사각형으로 고른다. `crossing` 이면 걸치기만 해도, 아니면 완전히 들어와야.
    ///
    /// 오토캐드에서 왼→오 드래그가 "포함", 오→왼이 "걸침"이다. 그 구분을
    /// 부르는 쪽이 정해 넘긴다.
    fn pick_window(
        &self, layout: &str, x0: f64, y0: f64, x1: f64, y1: f64, crossing: bool,
    ) -> usize {
        let bx = cadcraft_geom::Bounds2 {
            min: cadcraft_geom::Vec2 { x: x0.min(x1), y: y0.min(y1) },
            max: cadcraft_geom::Vec2 { x: x0.max(x1), y: y0.max(y1) },
        };
        cadcraft_engine::select::select_window(self.drawing(), &Self::space_of(layout), bx, crossing)
            .len()
    }

    /// 커서 근처에서 스냅 점을 찾는다.
    ///
    /// `osmode` 는 오토캐드 OSMODE 비트마스크다 — 1 끝점, 2 중간점, 4 중심,
    /// 32 교차점, 128 수직, 512 근처점, 4096 연장… Bimove 의 스냅 패널 토글이
    /// 그대로 비트 하나씩에 대응한다. 손으로 구현한 snap.ts 를 이걸로 대체한다.
    ///
    /// `base` 는 지금 그리는 선의 시작점 (수직·연장이 그걸 필요로 한다).
    fn snap(
        &self, layout: &str, x: f64, y: f64, aperture: f64, osmode: u32,
        base: Option<(f64, f64)>,
    ) -> Option<Snap> {
        let hit = cadcraft_engine::snap::osnap(
            self.drawing(),
            &Self::space_of(layout),
            cadcraft_geom::Vec2 { x, y },
            aperture,
            osmode,
            base.map(|(bx, by)| cadcraft_geom::Vec2 { x: bx, y: by }),
            false,
        )?;
        Some(Snap { x: hit.point.x, y: hit.point.y, name: hit.name, mode: hit.mode })
    }
}

/// 폰트를 이름으로 등록한다.
///
/// **브라우저에서는 이게 유일한 길이다.** CADCraft 는 빌드할 때 `CRAFT_FONTS_DIR`
/// 로 폰트를 박거나, 없으면 **시스템에 깔린 폰트**를 뒤진다. wasm 에는 시스템
/// 폰트가 없으니 아무것도 못 찾고 글자가 전부 네모(□)로 나온다 — 한의 도면을
/// 열어보니 제목이 통째로 □□□ 였다.
///
/// 그래서 JS 가 폰트 파일을 받아 여기로 넣어준다. Bimove 는 어차피 Noto Sans KR
/// 을 자체 호스팅하기로 했으니 (CDN 이 막힌 현장 망에서도 떠야 한다) 그 파일을
/// 그대로 쓰면 된다.
fn register_font_bytes(name: &str, bytes: Vec<u8>) {
    cadcraft_fonts::ttf::register(name, bytes);
}

/// 글자에 없는 문자를 찾을 때 뒤질 폰트 차례 (쉼표로 구분).
///
/// 등록만 하고 이걸 안 부르면, 도면이 지정한 폰트에 한글이 없을 때 여전히
/// □ 가 나온다. 등록된 이름을 여기 올려야 폴백으로 쓰인다.
fn set_fallbacks(list: &str) {
    cadcraft_fonts::ttf::set_fallback_fonts(list);
}

/// 열어둔 도면 하나.
///
/// 엔진의 `Session` 을 들고 있다. 그냥 `Drawing` 만 들면 읽기는 되지만 편집이
/// 안 된다 — 명령·되돌리기·선택이 전부 Session 에 매달려 있다.
pub struct Doc {
    session: Session,
    /// Bimove 문서에서 왔을 때의 페이지 목록 `(id, 이름)`. DXF 에서 왔으면 빈 배열.
    pages: Vec<(String, String)>,
    /// 옮기면서 **그리지 않은 것**. 조용히 빼면 "다 나왔다" 로 보인다.
    notes: Vec<String>,
    /// Bimove 문서에서 만든 선/글자/해치 수 (상태줄에 띄운다).
    counts: Counts,
    /// 실제로 옮긴 페이지 id. "" 를 넘겼을 때 **무엇이 골라졌는지** 알려준다.
    page_id: String,
    /// 마지막으로 만든 DisplayList 와 그때의 (공간, tolerance).
    ///
    /// **이게 성능의 전부다.** 매 프레임 build 하면 45MB 도면에서 한 장이
    /// 240ms 인데, 그중 래스터는 15ms 뿐이고 나머지가 build 다. 팬은 카메라만
    /// 움직이므로 같은 목록을 다시 쓰면 된다.
    /// (공간, tolerance, 배경, **도면 판번호**) → 그릴 거리.
    ///
    /// revision 을 키에 넣는 게 편집과 캐시를 잇는 전부다. 엔진이 도면을 고칠
    /// 때마다 올려주므로, 선 하나만 그어도 다음 렌더에서 자동으로 다시 만든다.
    /// 이걸 빼면 그린 게 화면에 안 나타난다.
    cache: Option<(String, f64, u32, u64, DisplayList)>,
}

/// Bimove 문서에서 **무엇이 몇 개 나왔는지.**
///
/// 상태줄에 그대로 띄우는 숫자다. 튜플이 아니라 이름을 붙여 둔다 — `counts.3`
/// 과 `counts.4` 가 벽인지 MLINE 인지 읽는 쪽에서 알 수가 없다.
#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    lines: usize,
    texts: usize,
    hatches: usize,
    /// 읽은 벽체 도형 수.
    walls: usize,
    /// 그 벽을 묶어 만든 MLINE 수. `walls` 보다 작아야 묶기가 듣는 것이다.
    wall_mlines: usize,
}

/// tolerance 가 이 배수 안쪽이면 다시 만들지 않는다.
///
/// tolerance 는 "화면 반 픽셀"이라 줌에 반비례한다. 조금만 달라도 다시 만들면
/// 휠을 굴릴 때마다 build 가 돌아 끊긴다. 2배까지는 곡선이 눈에 띄게 거칠어지지
/// 않으므로 그 안에서는 쓰던 걸 쓴다.
const TOL_SLACK: f64 = 2.0;

impl Doc {
    fn open(bytes: &[u8]) -> Result<Doc, String> {
        let drawing = cadcraft_io::read_dxf(bytes).map_err(|e| e.to_string())?;
        Ok(Doc::of(drawing, Vec::new(), Vec::new(), Counts::default(), String::new()))
    }

    /// Bimove 문서(우리 포맷 JSON)를 읽는다.
    ///
    /// DXF 를 거치지 않는다. 저장된 도면을 DXF 로 다시 써서 읽히는 길도 있지만,
    /// 그 왕복에서 색·레이어·글자 맞춤이 한 번 더 뭉개진다 — 그리고 애초에
    /// 지금 저장된 건 DXF 가 아니라 평평해진 선분이라 되돌릴 원본이 없다.
    pub fn from_bimove(json: &str, page_id: &str) -> Result<Doc, String> {
        let load = bimove::drawing_from_bimove(json, page_id)?;
        let counts = Counts {
            lines: load.line_count,
            texts: load.text_count,
            hatches: load.hatch_count,
            walls: load.wall_count,
            wall_mlines: load.wall_mline_count,
        };
        Ok(Doc::of(load.drawing, load.pages, load.notes, counts, load.page_id))
    }

    fn of(
        drawing: cadcraft_doc::Drawing, pages: Vec<(String, String)>, notes: Vec<String>,
        counts: Counts, page_id: String,
    ) -> Doc {
        let mut session = Session::empty();
        session.open_drawing(drawing, "bimove", None);
        Doc { session, pages, notes, counts, page_id, cache: None }
    }

    fn drawing(&self) -> &cadcraft_doc::Drawing {
        // open() 이 반드시 하나를 열어두므로 비어 있을 수 없다.
        self.session.doc().expect("도면이 열려 있지 않다")
    }

    /// 지금 도면의 판번호. 편집할 때마다 올라간다.
    fn revision(&self) -> u64 {
        self.session.state().map(|s| s.revision).unwrap_or(0)
    }

    /// 명령을 실행한다 — `id` 는 엔진 명령 이름("line", "erase", "u", "redo"…),
    /// `params` 는 그 명령이 받는 JSON.
    ///
    /// 되돌리기 스냅샷은 엔진이 알아서 쌓는다 (`execute` 안에서 push_undo).
    /// 그래서 우리가 실행취소를 따로 만들 필요가 없다 — `u` / `redo` 명령이
    /// 같은 길로 들어온다.
    fn exec(&mut self, id: &str, params_json: &str) -> Result<String, String> {
        let params: serde_json::Value = if params_json.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(params_json).map_err(|e| format!("params 가 JSON 이 아니다: {e}"))?
        };
        let v = self.session.execute(id, &params).map_err(|e| e.to_string())?;
        Ok(v.to_string())
    }

    /// 배치(종이공간) 탭 이름들. 모형 탭은 여기 없다 — `Space::Model` 로 따로 연다.
    fn layout_names(&self) -> Vec<String> {
        self.drawing().layouts.iter().map(|l| l.name.clone()).collect()
    }

    /// `layout` 이 비어 있으면 모형공간, 아니면 그 이름의 배치.
    ///
    /// `tolerance` 는 곡선을 선분으로 쪼갤 때의 오차(도면 단위)다. 화면 반 픽셀쯤이
    /// 적당하다 — 줌 배율에서 계산해 넘긴다. 너무 작게 주면 원 하나가 선분 수천 개가
    /// 된다.
    fn build(&self, layout: &str, tolerance: f64) -> Frame {
        let space = if layout.is_empty() {
            Space::Model
        } else {
            Space::Paper(layout.to_string())
        };
        let opts = Options { tolerance, ..Options::default() };
        Frame::from_list(&cadcraft_render::build(self.drawing(), &space, &opts))
    }
}

// ── JS 쪽에 내보내는 면 ──
//
// 위의 순수 Rust 를 그대로 감싸기만 한다. 로직을 여기 두지 않는 건 네이티브에서
// 같은 코드를 테스트할 수 있게 하기 위해서다 (wasm 빌드해야만 돌려볼 수 있으면
// 고치는 속도가 확 떨어진다).
/// 폰트를 등록한다. 도면을 열기 **전에** 불러야 한다.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = registerFont)]
pub fn register_font(name: &str, bytes: Vec<u8>) {
    register_font_bytes(name, bytes);
}

/// 폴백 폰트 차례를 정한다 (쉼표 구분). 등록한 이름을 여기 올려야 쓰인다.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = setFallbackFonts)]
pub fn set_fallback_fonts(list: &str) {
    set_fallbacks(list);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct BimoveDoc(Doc);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct BimoveFrame(Frame);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BimoveDoc {
    /// DXF 바이트를 읽는다 (ASCII/바이너리 둘 다).
    #[wasm_bindgen(js_name = openDxf)]
    pub fn open_dxf(bytes: &[u8]) -> Result<BimoveDoc, JsError> {
        Doc::open(bytes).map(BimoveDoc).map_err(|e| JsError::new(&e))
    }

    /// Bimove 문서(우리 포맷 JSON)를 읽는다. `page_id` 가 "" 면 첫 페이지.
    ///
    /// tldraw 로 저장된 도면을 엔진으로 그려보는 길이다. `pageIds`/`pageNames`
    /// 로 다른 페이지를 고르고, **`notes` 로 안 그린 것을 받는다** — 해치와
    /// Bimove 고유 도형(벽체·공간·치수…)은 아직 안 나온다.
    #[wasm_bindgen(js_name = fromBimove)]
    pub fn from_bimove(json: &str, page_id: &str) -> Result<BimoveDoc, JsError> {
        Doc::from_bimove(json, page_id).map(BimoveDoc).map_err(|e| JsError::new(&e))
    }

    /// Bimove 문서의 페이지 id 들 (DXF 에서 열었으면 빈 배열).
    #[wasm_bindgen(js_name = pageIds)]
    pub fn page_ids(&self) -> Vec<String> {
        self.0.pages.iter().map(|(id, _)| id.clone()).collect()
    }

    /// 같은 차례의 페이지 이름들.
    #[wasm_bindgen(js_name = pageNames)]
    pub fn page_names(&self) -> Vec<String> {
        self.0.pages.iter().map(|(_, name)| name.clone()).collect()
    }

    /// 옮기면서 **그리지 않은 것**. 비어 있으면 다 그렸다는 뜻이다.
    pub fn notes(&self) -> Vec<String> {
        self.0.notes.clone()
    }

    /// 지금 보고 있는 페이지 id. "" 를 넘겼으면 첫 페이지가 여기 들어 있다.
    #[wasm_bindgen(js_name = pageId)]
    pub fn page_id(&self) -> String {
        self.0.page_id.clone()
    }

    /// Bimove 문서에서 만든 선 개수.
    #[wasm_bindgen(js_name = lineCount)]
    pub fn line_count(&self) -> usize {
        self.0.counts.lines
    }

    /// Bimove 문서에서 만든 글자 개수.
    #[wasm_bindgen(js_name = textCount)]
    pub fn text_count(&self) -> usize {
        self.0.counts.texts
    }

    /// Bimove 문서에서 만든 해치 개수.
    #[wasm_bindgen(js_name = hatchCount)]
    pub fn hatch_count(&self) -> usize {
        self.0.counts.hatches
    }

    /// 읽은 벽체 **도형** 수.
    #[wasm_bindgen(js_name = wallCount)]
    pub fn wall_count(&self) -> usize {
        self.0.counts.walls
    }

    /// 벽체로 만든 MLINE 수.
    ///
    /// `wallCount` 와 **같이** 띄워야 쓸모가 있다. 이어진 벽이 묶이면 이 쪽이
    /// 훨씬 작고, 둘이 같으면 하나도 못 묶었다는 뜻이다 — 그러면 코너마다
    /// 캡이 서서 "꺾이는 지점이 겹친" 옛 모양으로 돌아간다.
    #[wasm_bindgen(js_name = wallMlineCount)]
    pub fn wall_mline_count(&self) -> usize {
        self.0.counts.wall_mlines
    }

    /// 배치 탭 이름들.
    #[wasm_bindgen(js_name = layoutNames)]
    pub fn layout_names(&self) -> Vec<String> {
        self.0.layout_names()
    }

    /// 그릴 거리를 만든다. `layout` 이 "" 면 모형공간.
    #[wasm_bindgen(js_name = buildFrame)]
    pub fn build_frame(&self, layout: &str, tolerance: f64) -> BimoveFrame {
        BimoveFrame(self.0.build(layout, tolerance))
    }

    /// 엔진 명령을 실행한다. 결과 JSON 을 돌려주고, 실패하면 throw 한다.
    ///
    /// 명령을 하나씩 바인딩하지 않고 **문 하나**로 연다 — 엔진에 명령이 수백 개라
    /// 하나씩 감싸면 끝이 없고, 늘 때마다 바인딩을 또 고쳐야 한다.
    ///
    /// ```js
    /// doc.exec('line',  JSON.stringify({ points: [[0,0],[100,50]] }))
    /// doc.exec('pline', JSON.stringify({ vertices: [[0,0],[10,0],[10,10]], closed: true }))
    /// doc.exec('move',  JSON.stringify({ handles: [h], delta: [100, 0] }))
    /// doc.exec('erase', JSON.stringify({ handles: [h] }))
    /// doc.exec('u',     '{}')            // 되돌리기
    /// doc.exec('redo',  '{}')            // 다시 실행
    /// ```
    ///
    /// 되돌리기 스냅샷은 엔진이 알아서 쌓는다. 우리가 실행취소를 따로 만들
    /// 필요가 없다 — `u` / `redo` 도 그냥 명령이다.
    pub fn exec(&mut self, id: &str, params_json: &str) -> Result<String, JsError> {
        self.0.exec(id, params_json).map_err(|e| JsError::new(&e))
    }

    /// 도면 판번호. 편집할 때마다 올라간다 — UI 가 "다시 그려야 하나" 를 이걸로 안다.
    pub fn revision(&self) -> f64 {
        self.0.revision() as f64
    }

    /// 한 점을 찍어 엔티티를 고른다. `aperture` 는 **도면 단위** (픽셀 ÷ 줌).
    pub fn pick(&self, layout: &str, x: f64, y: f64, aperture: f64) -> Option<String> {
        self.0.pick(layout, x, y, aperture)
    }

    /// 사각형 선택. `crossing` = 걸치기만 해도 잡기 (오→왼 드래그).
    #[wasm_bindgen(js_name = pickWindow)]
    pub fn pick_window(
        &self, layout: &str, x0: f64, y0: f64, x1: f64, y1: f64, crossing: bool,
    ) -> usize {
        self.0.pick_window(layout, x0, y0, x1, y1, crossing)
    }

    /// 커서가 붙을 점. `osmode` 는 오토캐드 OSMODE 비트마스크.
    /// `baseX`/`baseY` 가 NaN 이면 시작점 없음으로 본다 (수직·연장이 그걸 본다).
    pub fn snap(
        &self, layout: &str, x: f64, y: f64, aperture: f64, osmode: u32,
        base_x: f64, base_y: f64,
    ) -> Option<BimoveSnap> {
        let base = if base_x.is_nan() || base_y.is_nan() { None } else { Some((base_x, base_y)) };
        self.0.snap(layout, x, y, aperture, osmode, base).map(BimoveSnap)
    }

    /// 픽셀로 그린다. `cx`,`cy` 는 화면 가운데 도면 좌표, `scale` 은 단위당 픽셀.
    /// `bg` 는 0xRRGGBB. 크기가 0 이거나 터무니없으면 null.
    /// `draft` 면 **그릴 거리를 다시 만들지 않는다** — 쓰던 걸 그대로 쓴다.
    /// 휠·드래그 중에 켜라. 60만 선분짜리에서 목록 다시 만들기가 프레임당
    /// 150ms 라, 그 프레임이 통째로 밀리면서 직전 그림이 남아 보인다.
    /// 손을 떼면 draft 를 끄고 한 번 더 그려서 제대로 맞춘다.
    #[wasm_bindgen(js_name = renderPixels)]
    #[allow(clippy::too_many_arguments)]
    pub fn render_pixels(
        &mut self, layout: &str, cx: f64, cy: f64, scale: f64, w: u32, h: u32, bg: u32,
        draft: bool,
    ) -> Option<BimoveRaster> {
        self.0.raster(layout, cx, cy, scale, w, h, bg, draft).map(BimoveRaster)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct BimoveSnap(Snap);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BimoveSnap {
    pub fn x(&self) -> f64 {
        self.0.x
    }
    pub fn y(&self) -> f64 {
        self.0.y
    }
    /// "Endpoint" / "Midpoint" / "Perpendicular" … 마커 옆에 그대로 띄우면 된다.
    pub fn name(&self) -> String {
        self.0.name.to_string()
    }
    /// 걸린 OSMODE 비트 하나.
    pub fn mode(&self) -> u32 {
        self.0.mode
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct BimoveRaster(Raster);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BimoveRaster {
    /// `ImageData` 에 그대로 넣을 RGBA 바이트 (곱하지 않은 알파).
    pub fn rgba(&self) -> Vec<u8> {
        self.0.rgba.clone()
    }
    pub fn width(&self) -> u32 {
        self.0.w
    }
    pub fn height(&self) -> u32 {
        self.0.h
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BimoveFrame {
    /// 프리미티브 하나당 정수 개수. TS 의 읽는 루프가 이 값을 쓴다.
    #[wasm_bindgen(js_name = primStride)]
    pub fn prim_stride() -> usize {
        PRIM_STRIDE
    }

    /// x,y 가 번갈아 들어간 좌표 배열.
    pub fn verts(&self) -> Vec<f64> {
        self.0.verts.clone()
    }

    /// 삼각형 좌표 (세 꼭짓점이 이어서 온다).
    pub fn tris(&self) -> Vec<f64> {
        self.0.tris.clone()
    }

    /// `[kind, rgb, start, len, lw_um, flags]` 의 반복.
    pub fn prims(&self) -> Vec<i32> {
        self.0.prims.clone()
    }

    /// `[minX, minY, maxX, maxY]`
    pub fn bounds(&self) -> Vec<f64> {
        self.0.bounds.to_vec()
    }

    /// 프리미티브 개수 (= prims().length / primStride).
    #[wasm_bindgen(js_name = primCount)]
    pub fn prim_count(&self) -> usize {
        self.0.prims.len() / PRIM_STRIDE
    }

    /// 펼친 블록 수. 상한에 걸려 잘렸는지 보는 용도다.
    pub fn expanded(&self) -> usize {
        self.0.expanded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 가장 단순한 DXF — 선 하나.
    fn one_line_dxf() -> Vec<u8> {
        let mut s = String::new();
        for (c, v) in [
            ("0", "SECTION"), ("2", "ENTITIES"),
            ("0", "LINE"), ("8", "0"),
            ("10", "0.0"), ("20", "0.0"), ("30", "0.0"),
            ("11", "100.0"), ("21", "50.0"), ("31", "0.0"),
            ("0", "ENDSEC"), ("0", "EOF"),
        ] {
            s.push_str(c);
            s.push('\n');
            s.push_str(v);
            s.push('\n');
        }
        s.into_bytes()
    }

    #[test]
    fn 선_하나를_읽어_그릴_거리로_만든다() {
        let doc = Doc::open(&one_line_dxf()).expect("DXF 를 못 읽었다");
        let f = doc.build("", 0.001);
        assert_eq!(f.prims.len() % PRIM_STRIDE, 0, "프리미티브 배열이 stride 로 안 나눠진다");
        assert!(!f.prims.is_empty(), "선 하나짜리 도면인데 그릴 게 없다");
        assert_eq!(f.prims[0], KIND_POLYLINE, "LINE 은 폴리라인으로 와야 한다");
        // 좌표가 x,y 쌍으로 평평하게 들어갔는지
        assert_eq!(f.verts.len() % 2, 0);
        assert!(f.verts.len() >= 4, "선 하나면 점이 둘은 있어야 한다");
    }

    #[test]
    fn 범위가_도면_좌표와_맞는다() {
        // (0,0)~(100,50) 선 하나니까 bbox 도 그래야 한다. 여기가 어긋나면
        // 화면 맞춤(zoom to fit)이 엉뚱한 데를 비춘다.
        let doc = Doc::open(&one_line_dxf()).unwrap();
        let f = doc.build("", 0.001);
        assert!((f.bounds[0] - 0.0).abs() < 1e-6, "minX {}", f.bounds[0]);
        assert!((f.bounds[1] - 0.0).abs() < 1e-6, "minY {}", f.bounds[1]);
        assert!((f.bounds[2] - 100.0).abs() < 1e-6, "maxX {}", f.bounds[2]);
        assert!((f.bounds[3] - 50.0).abs() < 1e-6, "maxY {}", f.bounds[3]);
    }

    #[test]
    fn 픽셀로_그리면_선이_실제로_찍힌다() {
        // 크기만 맞고 전부 배경색이면 "그려진 척" 이다. 배경이 아닌 픽셀이
        // 있어야 진짜 그린 것이다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let (w, h) = (200, 100);
        // (0,0)~(100,50) 선을 가운데(50,25)에 두고 1.5px/단위로
        let r = doc.raster("", 50.0, 25.0, 1.5, w, h, 0xFFFFFF, false).expect("래스터 실패");
        assert_eq!(r.w, w);
        assert_eq!(r.h, h);
        assert_eq!(r.rgba.len(), (w as usize) * (h as usize) * 4, "RGBA 길이가 안 맞는다");

        let bg = r.rgba.chunks_exact(4).filter(|p| p[0] == 255 && p[1] == 255 && p[2] == 255).count();
        let ink = (w as usize) * (h as usize) - bg;
        assert!(ink > 20, "배경이 아닌 픽셀이 {ink}개뿐 — 선이 안 찍혔다");
        assert!(bg > 0, "전부 잉크다 — 배경색이 안 먹었다");
    }

    // ── 화면에서 구분되지 않는 점 덜어내기 ──
    //
    // 이건 **그려지는 픽셀을 바꾸는** 변경이라, "빨라졌다" 만으로는 부족하다.
    // Douglas–Peucker 의 보장(간략화한 선이 원래 선에서 ε 안)이 실제로 서는지,
    // 그리고 그릴 거리의 짜임(프리미티브 번호로 서로를 가리키는 구조)이
    // 안 깨지는지를 본다.

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2 { x, y }
    }

    /// 원래 점들이 간략화한 선에서 얼마나 멀어졌나 (최대).
    fn max_deviation(orig: &[Vec2], keep: &[bool]) -> f64 {
        let kept: Vec<Vec2> =
            orig.iter().zip(keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect();
        orig.iter()
            .map(|p| {
                kept.windows(2)
                    .map(|w| perp_dist(*p, w[0], w[1]))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max)
    }

    #[test]
    fn 직선_위의_중간점은_사라진다() {
        let pts = [v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0), v(3.0, 0.0), v(4.0, 0.0)];
        let mut keep = Vec::new();
        douglas_peucker(&pts, 0.5, &mut keep);
        assert_eq!(keep, vec![true, false, false, false, true]);
    }

    #[test]
    fn 튀는_점은_남는다() {
        // ε 보다 멀리 벗어난 점을 지우면 **선 모양이 달라진다.**
        let pts = [v(0.0, 0.0), v(2.0, 5.0), v(4.0, 0.0)];
        let mut keep = Vec::new();
        douglas_peucker(&pts, 0.5, &mut keep);
        assert_eq!(keep, vec![true, true, true]);
    }

    #[test]
    fn 양_끝은_언제나_남는다() {
        // 끝점을 잃으면 선이 짧아진다 — 도면에서는 벽이 안 닿는 걸로 보인다.
        let pts = [v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0)];
        let mut keep = Vec::new();
        douglas_peucker(&pts, 100.0, &mut keep);
        assert!(keep[0] && keep[2]);
    }

    #[test]
    fn 어긋남이_ε_를_넘지_않는다() {
        // 이게 이 변경의 **보장 그 자체**다. 깨지면 도면이 눈에 띄게 달라진다.
        let mut r = 12_345_u64;
        let mut next = || {
            r = r.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            ((r >> 33) as f64) / ((1u64 << 31) as f64)
        };
        for eps in [0.1, 0.5, 2.0, 10.0] {
            let pts: Vec<Vec2> =
                (0..400).map(|i| v(f64::from(i) * 0.7, (next() - 0.5) * 20.0)).collect();
            let mut keep = Vec::new();
            douglas_peucker(&pts, eps, &mut keep);
            let dev = max_deviation(&pts, &keep);
            assert!(dev <= eps + 1e-9, "ε {eps} 인데 {dev} 만큼 어긋났다");
        }
    }

    #[test]
    fn 닫힌_선도_다룬다() {
        // 첫 점과 끝 점이 같으면 기준선이 길이 0 이다. 거기서 0 으로 나누면
        // NaN 이 번져서 도면이 통째로 사라진다.
        let pts = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0), v(0.0, 0.0)];
        let mut keep = Vec::new();
        douglas_peucker(&pts, 0.5, &mut keep);
        assert_eq!(keep.iter().filter(|k| **k).count(), 5, "사각형 꼭짓점이 사라졌다");
    }

    #[test]
    fn 덜어내도_프리미티브_수는_그대로다() {
        // 글자(texts[].prim)와 레이어(prim_layers)가 프리미티브 **번호**로
        // 서로를 가리킨다. 하나라도 빼면 그 참조가 전부 어긋난다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let before = doc.build("", 0.001).prims.len();
        // 거칠게 그려도(=많이 덜어내도) 개수는 안 변해야 한다
        doc.raster("", 0.0, 0.0, 0.001, 64, 64, 0xFFFFFF, false);
        let after = doc.build("", 0.001).prims.len();
        assert_eq!(before, after);
    }

    #[test]
    fn ε_가_말이_안_되면_아무것도_안_한다() {
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let (a, b) = doc.decimate_report("", bad);
            assert_eq!(a, b, "ε {bad} 인데 점을 건드렸다");
        }
    }

    #[test]
    fn 덜어내도_그려지는_건_그대로다() {
        // 숫자만 줄고 화면이 비면 아무 소용이 없다. 잉크 픽셀이 남아 있는지
        // 본다 (덜어내기는 반 픽셀 안쪽만 건드리므로 크게 달라질 수 없다).
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let r = doc.raster("", 50.0, -25.0, 1.5, 200, 100, 0xFFFFFF, false).unwrap();
        let ink = r.rgba.chunks_exact(4).filter(|p| !(p[0] == 255 && p[1] == 255 && p[2] == 255)).count();
        assert!(ink > 20, "덜어내고 나니 {ink} 픽셀뿐 — 도면이 사라졌다");
    }

    // ── Bimove 문서로 여는 길 ──

    /// dxfgroup 하나짜리 Bimove 문서. 선 하나 + 글자 하나.
    fn bimove_json() -> String {
        r##"{"version":1,
          "pages":[{"id":"page:main","name":"모형","index":"a1"}],
          "shapes":[{"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1",
            "parentId":"page:main",
            "props":{"pathData":"M0,0L100,50",
                     "textsJson":"[{\"x\":10,\"y\":10,\"t\":\"거실\",\"h\":8}]",
                     "hatchesJson":"[{}]"},
            "meta":{"dxfLayer":"A-WALL","dxfColor":"#ff0000"}},
            {"id":"shape:w","type":"wall","x":0,"y":60,"rotation":0,"index":"a2",
             "parentId":"page:main","props":{"x2":100,"y2":0,"thickness":20}}],
          "records":{},"skipped":0,
          "source":{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":2}}"##
            .to_string()
    }

    #[test]
    fn bimove_문서를_열어_픽셀로_그린다() {
        // 여기까지 통하면 렌더·선택·스냅·편집이 전부 같은 길로 붙는다.
        // 배경색뿐이면 "그려진 척" 이므로 잉크를 센다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let (w, h) = (200, 100);
        // 캔버스 y 가 뒤집히므로 도면은 y = -50..0 에 있다
        let r = doc.raster("", 50.0, -25.0, 1.5, w, h, 0xFFFFFF, false).expect("래스터 실패");
        let bg = r.rgba.chunks_exact(4).filter(|p| p[0] == 255 && p[1] == 255 && p[2] == 255).count();
        let ink = (w as usize) * (h as usize) - bg;
        assert!(ink > 20, "배경이 아닌 픽셀이 {ink}개뿐 — 아무것도 안 찍혔다");
    }

    #[test]
    fn bimove_문서의_범위가_y를_뒤집은_자리에_있다() {
        // 안 뒤집으면 도면이 화면 밖(위아래 반대)에 생겨서 "빈 화면" 으로 보인다.
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let f = doc.build("", 1.0);
        assert!(f.bounds[1] < 0.0 && f.bounds[3] <= 0.0, "y 범위 {:?}", f.bounds);
    }

    #[test]
    fn bimove_문서는_안_그린_것을_알려준다() {
        // 조용히 빼면 "다 나왔다" 로 보인다.
        //
        // 이 목록은 **줄어드는 쪽으로만** 바뀐다. 해치가 먼저 빠졌고 이번에
        // 벽체가 빠졌다. 그래서 "무엇이 없는가" 로 적는다 — 있는 것만 적으면
        // 다음에 하나를 옮겼을 때 이 테스트가 아무것도 안 지킨다.
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let notes = doc.notes.join(" | ");
        assert!(!notes.contains("해치"), "해치는 그린다: {notes}");
        assert!(!notes.contains("wall"), "벽체는 그린다: {notes}");
        assert!(!notes.contains("못 그렸다"), "성한 도형을 빼먹었다: {notes}");
        assert_eq!(doc.pages, vec![("page:main".to_string(), "모형".to_string())]);
    }

    #[test]
    fn bimove_문서의_벽이_mline_으로_들어간다() {
        // 벽을 MLINE 으로 넣는 게 코너가 맞물리는 이유다. 폴리라인으로 넣으면
        // miter 를 손으로 계산해야 하고, 그게 tldraw 쪽에서 세 번 틀렸다.
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let n = doc
            .drawing()
            .model
            .iter()
            .filter(|e| matches!(e.kind, cadcraft_doc::EntityKind::MLine(_)))
            .count();
        assert_eq!(n, 1, "MLINE 이 {n}개다");
        assert_eq!(doc.counts.walls, 1);
        assert_eq!(doc.counts.wall_mlines, 1);
    }

    #[test]
    fn bimove_문서에서도_편집과_되돌리기가_된다() {
        // DisplayList 를 직접 넘기는 길로는 안 되는 것. 엔티티로 바꾼 값이 여기다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let before = doc.build("", 1.0).prims.len();
        doc.exec("line", r#"{"points":[[0,0],[10,10]]}"#).unwrap();
        let after = doc.build("", 1.0).prims.len();
        assert!(after > before, "{before} → {after}");
        doc.exec("u", "{}").unwrap();
        assert_eq!(doc.build("", 1.0).prims.len(), before);
    }

    #[test]
    fn 해치가_실제로_찍힌다() {
        // 엔티티가 맞게 들어가도 **화면에 아무것도 안 나오면** 소용이 없다.
        // 경계만 있는 것(solid=false, 패턴 없음)과 견줘서, 패턴 선이 안을
        // 채우는지 잉크 픽셀 수로 본다.
        let doc_with = |hatches: &str| {
            format!(
                r#"{{"version":1,"pages":[{{"id":"page:main","name":"모형","index":"a1"}}],
                 "shapes":[{{"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1",
                   "parentId":"page:main","props":{{"pathData":"","hatchesJson":{}}}}}],
                 "records":{{}},"skipped":0,
                 "source":{{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":1}}}}"#,
                serde_json::Value::String(hatches.to_string())
            )
        };
        // **안쪽만** 센다. 테두리 선은 패턴이 하나도 없어도 찍히므로, 전체를
        // 세면 "경계만 그렸는데 다 된 줄" 아는 테스트가 된다 (실제로 그렇게
        // 써놨다가 패턴을 통째로 없애도 안 깨지는 걸 보고 고쳤다).
        let inner_ink = |json: &str| {
            let mut d = Doc::from_bimove(json, "").unwrap();
            let (w, h) = (200usize, 200usize);
            let r = d.raster("", 50.0, -50.0, 1.8, w as u32, h as u32, 0xFFFFFF, false).unwrap();
            let mut n = 0;
            for y in h / 4..h * 3 / 4 {
                for x in w / 4..w * 3 / 4 {
                    let p = &r.rgba[(y * w + x) * 4..][..4];
                    if !(p[0] == 255 && p[1] == 255 && p[2] == 255) {
                        n += 1;
                    }
                }
            }
            n
        };
        let ink = inner_ink;

        let hatched = ink(&doc_with(
            r#"[{"d":"M0,0L100,0L100,100L0,100Z","p":"ANSI31","dl":[{"a":45,"s":8}],"dim":100}]"#,
        ));
        let solid = ink(&doc_with(
            r#"[{"d":"M0,0L100,0L100,100L0,100Z","p":"SOLID","f":1,"dim":100}]"#,
        ));

        assert!(hatched > 200, "안쪽에 {hatched} 픽셀뿐 — 패턴이 안을 안 채웠다");
        assert!(solid > hatched, "솔리드({solid})가 패턴({hatched})보다 안 짙다");
    }

    #[test]
    fn 깨진_bimove_문서는_에러로_돌려준다() {
        assert!(Doc::from_bimove("나는 JSON 이 아니다", "").is_err());
    }

    #[test]
    fn 알파는_곱해지지_않은_채로_나온다() {
        // tiny-skia 는 알파가 곱해진 픽셀을 준다. 그대로 넘기면 캔버스에서
        // 색이 어두워진다. 불투명 배경이니 알파가 전부 255 여야 한다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let r = doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false).unwrap();
        assert!(r.rgba.chunks_exact(4).all(|p| p[3] == 255), "알파가 255 가 아닌 픽셀이 있다");
    }

    #[test]
    fn 팬은_목록을_다시_만들지_않는다() {
        // 카메라만 움직이면 같은 DisplayList 를 쓴다. 이게 깨지면 45MB 도면에서
        // 한 프레임이 15ms 에서 240ms 로 돌아간다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false).unwrap();
        let built = doc.cache.as_ref().map(|(_, t, _, _, dl)| (*t, dl.prims.len())).unwrap();
        doc.raster("", 999.0, -999.0, 1.5, 64, 64, 0xFFFFFF, false).unwrap();
        let after = doc.cache.as_ref().map(|(_, t, _, _, dl)| (*t, dl.prims.len())).unwrap();
        assert_eq!(built, after, "팬만 했는데 목록이 다시 만들어졌다");
    }

    #[test]
    fn 줌이_크게_바뀌면_다시_만든다() {
        // tolerance 는 줌에 반비례한다. 2배 안쪽은 그대로 쓰고, 넘으면 다시
        // 만들어야 곡선이 거칠어지지 않는다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.0, 64, 64, 0xFFFFFF, false).unwrap();
        let t0 = doc.cache.as_ref().unwrap().1;

        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false).unwrap();
        assert_eq!(doc.cache.as_ref().unwrap().1, t0, "1.5배 줌은 다시 만들 일이 아니다");

        doc.raster("", 50.0, 25.0, 10.0, 64, 64, 0xFFFFFF, false).unwrap();
        assert_ne!(doc.cache.as_ref().unwrap().1, t0, "10배 줌인데 그대로 썼다");
    }

    #[test]
    fn 공간이_바뀌면_다시_만든다() {
        // 모형 ↔ 배치는 아예 다른 내용이다. 캐시를 그대로 쓰면 엉뚱한 걸 그린다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false).unwrap();
        assert_eq!(doc.cache.as_ref().unwrap().0, "");
        doc.raster("배치1", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false).unwrap();
        assert_eq!(doc.cache.as_ref().unwrap().0, "배치1", "공간이 바뀌었는데 캐시가 그대로다");
    }

    // ── 흰 배경에서 읽히는 색 ──

    use cadcraft_color::Rgb;
    const WHITE: Rgb = Rgb(255, 255, 255);
    const DARK: Rgb = Rgb(33, 40, 48);   // CADCraft 기본 배경

    #[test]
    fn 흰_배경에서_노랑과_하늘이_읽히게_어두워진다() {
        // 한이 보낸 화면에서 안 보이던 바로 그 두 색이다.
        for c in [Rgb(255, 255, 0), Rgb(0, 255, 255)] {
            let f = readable_on(c, WHITE);
            // u8 로 반올림하면서 목표에서 한 칸(≈1/255) 모자랄 수 있다.
            assert!(
                WHITE.luma() - f.luma() >= MIN_INK_CONTRAST - 1.0 / 255.0,
                "{c:?} → {f:?} 인데 밝기 차가 {:.3} 뿐이다", WHITE.luma() - f.luma()
            );
        }
    }

    #[test]
    fn 색상은_보존된다() {
        // 노랑이 회색이 되면 레이어를 색으로 구분하던 사람이 길을 잃는다.
        let f = readable_on(Rgb(255, 255, 0), WHITE);
        assert_eq!(f.0, f.1, "R 과 G 가 같아야 노랑이다: {f:?}");
        assert_eq!(f.2, 0, "파랑이 섞였다: {f:?}");
        assert!(f.0 > 0, "까맣게 죽었다: {f:?}");

        let g = readable_on(Rgb(0, 255, 255), WHITE);
        assert_eq!(g.0, 0, "빨강이 섞였다: {g:?}");
        assert_eq!(g.1, g.2, "G 와 B 가 같아야 하늘이다: {g:?}");
    }

    #[test]
    fn 이미_어두운_색은_건드리지_않는다() {
        // 검정·남색은 흰 배경에서 이미 잘 보인다. 손대면 서로 구분이 사라진다.
        for c in [Rgb(0, 0, 0), Rgb(0, 0, 255), Rgb(165, 0, 0)] {
            assert_eq!(readable_on(c, WHITE), c, "{c:?} 를 건드렸다");
        }
    }

    #[test]
    fn 어두운_배경에서는_반대로_밝아진다() {
        // 같은 규칙이 검은 배경에도 서야 배경색을 바꿀 수 있다.
        let f = readable_on(Rgb(0, 0, 90), DARK);
        assert!(f.luma() - DARK.luma() >= MIN_INK_CONTRAST - 1.0 / 255.0, "{f:?}");
        assert!(f.2 >= 90, "밝히랬더니 어두워졌다: {f:?}");
        // 어두운 배경 쪽은 채도가 빠진다. 진한 파랑은 아무리 키워도 밝아지지
        // 않으므로(루마에서 파랑이 0.07 밖에 안 된다) 흰색을 섞는 수밖에 없다.
        // 흰 배경 — 한이 쓰는 쪽 — 은 색상이 보존된다. 위 테스트 참고.
    }

    #[test]
    fn 색_7번은_이미_뒤집힌_뒤에_판단한다() {
        // aci7 은 흰 배경에서 검정이 된다. 그걸 모르고 원본 흰색을 보면
        // "안 보인다" 고 판단해 멀쩡한 글씨를 회색으로 만든다.
        let mut dl = DisplayList::default();
        dl.prims.push(cadcraft_render::DPrim {
            handle: Default::default(),
            color: Rgb(255, 255, 255),
            aci7: true,
            lw: 0.0,
            transparency: 0,
            kind: Kind::Polyline,
            start: 0,
            len: 0,
        });
        make_readable(&mut dl, WHITE);
        assert!(dl.prims[0].aci7, "aci7 을 꺼버렸다 — 배경을 바꾸면 안 뒤집힌다");
    }

    #[test]
    fn 크기가_0_이면_실패를_돌려준다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        assert!(doc.raster("", 0.0, 0.0, 1.0, 0, 100, 0xFFFFFF, false).is_none());
    }

    // ── 선택 · 스냅 ──
    //
    // Bimove 의 snap.ts(끝점·중간점·교차점·수직·연장·직교)를 이걸로 대체한다.
    // 비트 하나씩이 패널 토글 하나에 대응하므로, 비트를 잘못 넘기면 조용히
    // 엉뚱한 데 붙는다. 여기서 비트별로 확인한다.

    /// (0,0)-(100,50) 선 하나. 중간점은 (50,25).
    const END: u32 = 1;
    const MID: u32 = 2;

    #[test]
    fn 선_위를_찍으면_골라진다() {
        let doc = Doc::open(&one_line_dxf()).unwrap();
        // 선 위 한 점 (중간 근처)
        assert!(doc.pick("", 50.0, 25.0, 1.0).is_some(), "선 위인데 안 골라졌다");
        // 한참 떨어진 곳
        assert!(doc.pick("", 50.0, 500.0, 1.0).is_none(), "멀리 찍었는데 골라졌다");
    }

    #[test]
    fn aperture_가_도면_단위라서_줌에_따라_달라진다() {
        // 화면 픽셀을 그대로 넘기면 줌인했을 때 클릭이 안 먹는다. 부르는 쪽이
        // 픽셀÷줌 으로 바꿔 넘겨야 한다는 계약을 여기서 못 박는다.
        let doc = Doc::open(&one_line_dxf()).unwrap();
        let off = (50.0, 30.0);  // 선에서 살짝 떨어진 점
        assert!(doc.pick("", off.0, off.1, 0.1).is_none(), "aperture 가 작은데 잡혔다");
        assert!(doc.pick("", off.0, off.1, 20.0).is_some(), "aperture 가 큰데 안 잡혔다");
    }

    #[test]
    fn 끝점_스냅이_선의_끝에_붙는다() {
        let doc = Doc::open(&one_line_dxf()).unwrap();
        let s = doc.snap("", 98.0, 48.0, 10.0, END, None).expect("끝점을 못 찾았다");
        assert!((s.x - 100.0).abs() < 1e-6 && (s.y - 50.0).abs() < 1e-6, "({},{})", s.x, s.y);
        assert_eq!(s.name, "Endpoint");
        assert_eq!(s.mode, END);
    }

    #[test]
    fn 중간점_스냅이_가운데에_붙는다() {
        let doc = Doc::open(&one_line_dxf()).unwrap();
        let s = doc.snap("", 52.0, 27.0, 10.0, MID, None).expect("중간점을 못 찾았다");
        assert!((s.x - 50.0).abs() < 1e-6 && (s.y - 25.0).abs() < 1e-6, "({},{})", s.x, s.y);
        assert_eq!(s.name, "Midpoint");
    }

    #[test]
    fn 끈_모드는_안_걸린다() {
        // 패널에서 끝점만 켜 뒀는데 중간점에 붙으면 사용자는 영문을 모른다.
        let doc = Doc::open(&one_line_dxf()).unwrap();
        let s = doc.snap("", 52.0, 27.0, 10.0, END, None);
        assert!(s.is_none_or(|h| h.mode == END), "끝점만 켰는데 다른 게 걸렸다");
    }

    #[test]
    fn 멀면_스냅이_없다() {
        let doc = Doc::open(&one_line_dxf()).unwrap();
        assert!(doc.snap("", 50.0, 900.0, 10.0, END | MID, None).is_none());
    }

    #[test]
    fn 사각형_선택_포함과_걸침이_다르다() {
        let doc = Doc::open(&one_line_dxf()).unwrap();
        // 선을 통째로 감싸는 상자 — 둘 다 잡아야 한다
        assert_eq!(doc.pick_window("", -10.0, -10.0, 110.0, 60.0, false), 1);
        assert_eq!(doc.pick_window("", -10.0, -10.0, 110.0, 60.0, true), 1);
        // 선을 가로지르기만 하는 작은 상자 — 걸침만 잡는다
        assert_eq!(doc.pick_window("", 40.0, 15.0, 60.0, 35.0, false), 0, "포함인데 잡혔다");
        assert_eq!(doc.pick_window("", 40.0, 15.0, 60.0, 35.0, true), 1, "걸침인데 안 잡혔다");
    }

    // ── 편집 ──
    //
    // 여기서 보는 건 "명령이 돌았나" 가 아니라 **그린 게 화면에 나오나** 다.
    // 도면이 바뀌어도 캐시가 그대로면 사용자는 아무 일도 안 일어난 걸로 본다.

    /// 지금 모형공간을 그리면 프리미티브가 몇 개인가.
    fn prim_count(doc: &mut Doc) -> usize {
        doc.raster("", 50.0, 25.0, 1.0, 64, 64, 0xFFFFFF, false).unwrap();
        doc.cache.as_ref().unwrap().4.prims.len()
    }

    #[test]
    fn 선을_그으면_도면에_늘어난다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let before = prim_count(&mut doc);
        doc.exec("line", r#"{"points": [[0,0],[0,200]]}"#).expect("line 실패");
        assert!(prim_count(&mut doc) > before, "선을 그었는데 안 늘었다");
    }

    #[test]
    fn 되돌리면_없어지고_다시하면_돌아온다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let base = prim_count(&mut doc);
        doc.exec("line", r#"{"points": [[0,0],[0,200]]}"#).unwrap();
        let drawn = prim_count(&mut doc);
        assert!(drawn > base);

        doc.exec("u", "{}").expect("되돌리기 실패");
        assert_eq!(prim_count(&mut doc), base, "되돌렸는데 그대로다");

        doc.exec("redo", "{}").expect("다시 실행 실패");
        assert_eq!(prim_count(&mut doc), drawn, "다시 했는데 안 돌아왔다");
    }

    #[test]
    fn 편집하면_캐시가_저절로_무효화된다() {
        // revision 을 캐시 키에 넣은 이유. 이게 깨지면 그린 게 화면에 안 나타난다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        prim_count(&mut doc);
        let r0 = doc.revision();
        doc.exec("line", r#"{"points": [[0,0],[0,200]]}"#).unwrap();
        assert!(doc.revision() > r0, "편집했는데 판번호가 그대로다");
        // 캐시에 남은 판번호가 옛것이라 다음 렌더가 다시 만든다
        assert_ne!(doc.cache.as_ref().unwrap().3, doc.revision());
    }

    #[test]
    fn 없는_명령은_에러다() {
        // 조용히 넘기면 "왜 아무 일도 안 일어나지" 가 된다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        assert!(doc.exec("이런명령없음", "{}").is_err());
    }

    #[test]
    fn 깨진_params_는_에러다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let e = doc.exec("line", "{이건 JSON 이 아니다}").unwrap_err();
        assert!(e.contains("JSON"), "에러에 이유가 없다: {e}");
    }

    #[test]
    fn 빈_params_는_빈_객체로_본다() {
        // JS 에서 '' 를 넘기는 실수가 흔하다. 거기서 터지면 원인을 못 찾는다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        assert!(doc.exec("u", "").is_ok());
    }

    #[test]
    fn 깨진_입력은_에러로_돌려준다() {
        // 조용히 빈 도면을 주면 "열렸는데 아무것도 없네" 가 되어 원인을 못 찾는다.
        assert!(Doc::open(b"this is not a dxf at all").is_err());
    }
}
