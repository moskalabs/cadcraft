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
pub mod boq;
pub mod dim;
pub mod grid;
pub mod identity;
pub mod jsnum;
pub mod opening;
pub mod to_bimove;
pub mod wall;
pub mod zone;

use cadcraft_doc::{EntityKind, Handle, Space};
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
        let opts = render_opts(tol, 0.0);
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
        self.raster(layout, cx, cy, scale, w, h, bg, draft, false, false)
    }

    fn raster(
        &mut self, layout: &str, cx: f64, cy: f64, scale: f64, w: u32, h: u32, bg: u32,
        draft: bool, grid: bool, gray: bool,
    ) -> Option<Raster> {
        let bg_rgb = cadcraft_color::Rgb((bg >> 16) as u8, (bg >> 8) as u8, bg as u8);
        // 곡선을 선분으로 쪼갤 오차는 "화면 반 픽셀". 줌이 바뀌면 같이 바뀐다.
        let tol = 0.5 / scale.max(1e-12);
        // 배경색도 키다 — 색 보정이 배경에 달려 있어서, 배경이 바뀌면 같은
        // 목록을 쓰면 안 된다.
        let rev = self.revision();
        let reusable = self.cache.as_ref().is_some_and(|(l, t, b, g, r, _)| {
            l == layout
                && *b == bg
                // 흑백은 **색을 통째로 갈아 끼운다.** 키에 안 넣으면 토글을
                // 눌러도 쓰던 목록을 그대로 써서 아무 일도 안 난다.
                && *g == gray
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
            // 점 크기는 **화면 높이**를 따라간다 (render_opts 주석).
            // `tol` 과 똑같이 1/scale 에 비례하므로, 위 캐시가 tol 을 봐주는
            // 범위(2배) 안에서 같이 움직인다 — 따로 키를 늘릴 필요가 없다.
            // 캔버스 높이가 2배 넘게 바뀌면 그만큼 어긋난 채로 한 번 더
            // 그려지는데, 다음 줌에서 맞춰진다.
            let opts = render_opts(tol, f64::from(h) / scale.max(1e-12));
            let mut dl = cadcraft_render::build(self.drawing(), &space, &opts);
            make_readable(&mut dl, bg_rgb, self.drawing());
            // **읽히게 한 다음에** 먹으로 바꾼다. 순서를 바꾸면 우리가 정한
            // 농도를 `make_readable` 이 다시 끌어내린다.
            if gray {
                let dark = (u32::from(bg_rgb.0) + u32::from(bg_rgb.1) + u32::from(bg_rgb.2)) / 3 < 128;
                to_gray(&mut dl, dark, self.drawing());
            }
            // **틀과 대각선을 걷어낸다.** `render` 크레이트는 `Image` 를 "그림을
            // 못 찾았다" 는 꼴로 그린다 (네모 + 대각선 둘). 우리는 그림을
            // 가졌으니 그 위에 대각선이 그어지면 안 된다 — 편집 화면도 밑그림을
            // 그냥 깔지 틀을 안 그린다.
            //
            // 프리미티브만 지운다. `verts`/`tris` 는 각 프리미티브가 start/len
            // 으로 가리키므로 **건드리면 안 된다** — 줄이면 남은 것들의 번호가
            // 어긋난다. 조금 남는 자리는 틀 세 줄뿐이다.
            if !self.underlays.is_empty() {
                let drawn: Vec<Handle> = self.underlays.iter().map(|u| u.handle).collect();
                dl.prims.retain(|p| !drawn.contains(&p.handle));
            }
            // 그릴 때 쓰는 tolerance 와 **같은 값**으로 덜어낸다. 그래야
            // 보이는 차이가 없다는 보장이 선다 (decimate 주석 참고).
            decimate(&mut dl, tol);
            self.cache = Some((layout.to_string(), tol, bg, gray, rev, dl));
        }
        let view = cadcraft_render::raster::View {
            center: cadcraft_geom::Vec2 { x: cx, y: cy },
            scale,
            width: w,
            height: h,
        };
        // 조작 중에도 **계단은 다듬는다.** 안티앨리어싱을 끄면 빨라질 것
        // 같지만 재보니 **느려졌다** — draft 평균 21.3 → 35.3ms, 최악
        // 97 → 230ms. tiny-skia 의 비-AA 경로가 얇은 선에서 더 비싸다.
        // 그 측정을 남겨 둔다; 다시 시도할 사람이 또 재지 않도록.
        let ro = cadcraft_render::raster::RasterOptions { background: bg_rgb, ..Default::default() };
        // 밑그림은 **도면 밑에** 깐다. 바탕을 깔고 → 그림을 얹고 → 그 위에
        // 도면을 그린다. 순서를 바꾸면 그림이 그 위에 그은 선을 덮어서,
        // 밑그림을 따라 그리는 일 자체가 안 된다.
        let mut pm = cadcraft_render::raster::canvas(&view, &ro)?;
        // **격자는 맨 밑이다.** 바탕 바로 위, 밑그림보다도 아래. 위에 그으면
        // 격자가 벽을 가로질러서 도면이 통째로 더러워 보인다.
        if grid {
            crate::grid::draw(&mut pm, &view, bg_rgb);
        }
        if !self.underlays.is_empty() {
            let d = self.drawing();
            for u in &self.underlays {
                blit_underlay(&mut pm, u, d, &view);
            }
        }
        let dl = &self.cache.as_ref()?.5;
        cadcraft_render::raster::render_into(&mut pm, dl, &view, &ro);

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

/// 이 삼각형 덩이가 **잉크**인가 — 바닥이 아니라 읽혀야 하는 것인가.
///
/// 글자 글리프와 화살촉은 외곽선을 쪼갠 삼각형(`Kind::Tris`)으로 나온다.
/// 그래서 면과 한 덩이로 묶어 건너뛰면 **어두운 배경에서 글자가 안 보인다** —
/// 지시선은 밝아졌는데 그 끝에 붙은 글만 묻힌 화면이 된다 (실제로 그렇게
/// 한 번 나왔다).
///
/// 반대로 물량표의 연한 띠나 공간의 비치는 바닥은 **면**이다. 그건 연한 게
/// 목적이라 끌어내면 숯색 덩어리가 되고, 그 위의 글자까지 못 읽게 된다.
/// 그래서 종류로 가린다 — 글자·치수·지시선에서 나온 삼각형만 잉크로 본다.
///
/// 블록 안에서 나온 것은 `model` 에 없어서 못 찾는다. 그때는 **안 밝힌다** —
/// 모르는 것을 밝혀서 바닥을 망가뜨리는 쪽이 더 나쁘다.
fn is_ink_tris(d: &cadcraft_doc::Drawing, h: cadcraft_doc::Handle) -> bool {
    d.model.get(h).is_some_and(|e| {
        matches!(
            e.kind,
            EntityKind::Text(_)
                | EntityKind::MText(_)
                | EntityKind::AttDef(_)
                | EntityKind::Dimension(_)
                | EntityKind::Leader(_)
                | EntityKind::MLeader(_)
        )
    })
}

/// 배경에 묻히는 **잉크**를 끌어낸다. **DisplayList 를 제자리에서 고친다.**
///
/// 채움(`Kind::Tris`)과 가림(`Kind::Mask`)은 건드리지 않는다. 면은 잉크가
/// 아니라 **바닥**이고, 연한 건 연한 게 목적이다 — 물량표의 제목·머리·합계
/// 띠(`#f1f3f4` 들)나 공간의 비치는 회색이 그렇다. 묻힌다고 끌어내리면 연한
/// 띠가 **숯색 덩어리**가 되어, 그 위에 얹힌 글자까지 못 읽게 된다. 읽혀야
/// 하는 건 면 위의 글자이고, 그건 여기서 따로 손본다.
///
/// 어두운 배경에서 어두운 면은 안 보이게 된다. 그건 받아들인다 — Bimove 의
/// 캔버스는 흰색이고, 면을 밝히면 흰 배경에서 띠가 사라진다.
fn make_readable(dl: &mut DisplayList, bg: cadcraft_color::Rgb, d: &cadcraft_doc::Drawing) {
    for p in &mut dl.prims {
        if matches!(p.kind, Kind::Mask) {
            continue;
        }
        // 면은 바닥이라 안 건드린다 — **글자에서 나온 삼각형만** 빼고.
        if matches!(p.kind, Kind::Tris) && !is_ink_tris(d, p.handle) {
            continue;
        }
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

/// 흑백(CAD) 보기 — 색을 **먹 농도**로 바꾼다.
///
/// 한: "여기 gray on off 원래 기능 있었는데 없네. 그거 넣자"
///
/// tldraw 화면에는 있었다 (`LayersPanel` 의 [Grayscale ON/OFF]). 엔진은 그
/// 설정을 안 봐서 단추를 아예 안 띄웠다 — 눌리는데 아무 일 없는 단추보다는
/// 나았지만, 기능은 빈 자리였다.
///
/// ── 왜 그냥 desaturate 가 아닌가 ──
///
/// 색이 사라진 자리를 **농도**가 대신해야 한다. 선과 글자를 같은 검정으로
/// 두면 글자가 선을 이긴다. 그래서 선 → 글자로 단을 둔다. 숫자는 화면 쪽
/// `lib/grayscale.ts` 와 **같은 값**이다 (17 / 51, 어두운 바탕 240 / 208) —
/// 두 화면이 다른 농도로 그리면 어느 쪽이 맞는지 알 수 없다.
///
/// ── 면은 **밝기를 지킨다** ──
///
/// 처음엔 면도 정해진 해치 먹(89)으로 칠했다. 그랬더니 **벽이 통째로 회색
/// 덩어리**가 됐다 — 벽 속을 채우는 흰 면까지 같이 칠해져서다. 그 흰 면은
/// 해치가 아니라 **바닥**이고, 흰 것이 목적이다.
///
/// `make_readable` 이 같은 자리에서 같은 말을 한다: "면은 잉크가 아니라
/// 바닥이고, 연한 건 연한 게 목적이다. 묻힌다고 끌어내리면 연한 띠가 숯색
/// 덩어리가 되어, 그 위에 얹힌 글자까지 못 읽게 된다."
///
/// 그래서 면은 **제 밝기 그대로 색만 뺀다.** 흰 면은 희게 남고, 짙은 해치는
/// 짙은 회색이 된다 — 원래의 단이 그대로 살아 있다.
fn to_gray(dl: &mut DisplayList, dark: bool, d: &cadcraft_doc::Drawing) {
    let (line, text) = if dark {
        (cadcraft_color::Rgb(0xf0, 0xf0, 0xf0), cadcraft_color::Rgb(0xd0, 0xd0, 0xd0))
    } else {
        (cadcraft_color::Rgb(0x11, 0x11, 0x11), cadcraft_color::Rgb(0x33, 0x33, 0x33))
    };
    for p in &mut dl.prims {
        // 가림막은 **바탕을 칠하는 것**이다. 먹으로 바꾸면 가리려던 자리가
        // 메워진다.
        if matches!(p.kind, Kind::Mask) {
            continue;
        }
        p.color = if is_ink_tris(d, p.handle) {
            text
        } else if matches!(p.kind, Kind::Tris) {
            luma(p.color)
        } else {
            line
        };
        // 우리가 정한 색을 그대로 쓰게 한다 — 안 그러면 7번 색 규칙이
        // 배경에 맞춰 다시 뒤집는다.
        p.aci7 = false;
    }
}

/// 색만 빼고 **밝기는 그대로** (Rec. 601).
fn luma(c: cadcraft_color::Rgb) -> cadcraft_color::Rgb {
    let y = 0.299 * f64::from(c.0) + 0.587 * f64::from(c.1) + 0.114 * f64::from(c.2);
    let v = y.round().clamp(0.0, 255.0) as u8;
    cadcraft_color::Rgb(v, v, v)
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

    /// 이 엔티티와 **같은 Bimove 객체**인 엔티티들의 손잡이 (그리는 차례로).
    ///
    /// 문 하나는 엔티티 다섯이다. 하나를 찍었을 때 나머지 넷을 찾는 길이
    /// 이것이다 — 안 그러면 문짝 선 하나만 골라 지울 수 있다.
    ///
    /// **정체성이 없으면 빈 배열이다.** 찍은 것을 그대로 돌려주지 않는다.
    /// 그러면 부르는 쪽이 "묶음이 있다" 와 "그냥 선 하나다" 를 구분할 수
    /// 없고, 그 둘은 화면에서 다르게 다뤄야 한다 ([[identity]] 참고).
    ///
    /// 훑기를 여기서 하는 이유는 양이다. 3,500 개 엔티티의 xdata 를 JSON 으로
    /// 경계 밖으로 내보내면 클릭마다 몇 MB 가 오간다.
    fn group(&self, handle_hex: &str) -> Vec<String> {
        let Some(h) = cadcraft_doc::Handle::parse_hex(handle_hex) else { return Vec::new() };
        let d = self.drawing();
        let Some(me) = d.model.get(h).and_then(|e| crate::identity::of(e)) else {
            return Vec::new();
        };
        let mine: std::collections::BTreeSet<&str> = me.ids.iter().map(String::as_str).collect();
        d.model
            .iter()
            // 종류까지 본다. 같은 Bimove 도형 id 를 다른 종류가 들 일은 없지만,
            // 그 전제가 깨지는 날 벽과 문이 한 묶음이 되는 쪽보다 안 묶이는
            // 쪽이 낫다.
            .filter(|e| {
                crate::identity::of(e)
                    .is_some_and(|g| g.kind == me.kind && g.ids.iter().any(|i| mine.contains(i.as_str())))
            })
            .map(|e| e.handle.hex())
            .collect()
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
        // **`{h:?}` 를 쓰면 안 된다.** 그건 "Handle(257)" 로 나오는데, 명령이
        // 받고 돌려주는 건 16진 문자열 "101" 이다. 그대로 내보내면 JS 에서
        // 고른 것을 `exec` 에 다시 넣는 길이 끊긴다 — 누르면 선택은 되는데
        // 옮기기·지우기가 "그런 handle 없다" 로 조용히 실패한다.
        Some(h.hex())
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
    /// **열 때** 도면에 심긴 Bimove 도형 id 집합.
    ///
    /// 저장할 때 지워진 것을 가리는 기준이다. 지금 도면에 있는 것만 보면
    /// 정방향이 조용히 빼는 도형(돌아간 그룹 안, 두께 0 인 벽, 다른 페이지)이
    /// "사용자가 지웠다" 와 구분되지 않아 **열지도 않은 도형이 사라진다**
    /// (`to_bimove` 모듈 주석).
    loaded_ids: std::collections::BTreeSet<String>,
    /// 마지막으로 만든 DisplayList 와 그때의 (공간, tolerance).
    ///
    /// **이게 성능의 전부다.** 매 프레임 build 하면 45MB 도면에서 한 장이
    /// 240ms 인데, 그중 래스터는 15ms 뿐이고 나머지가 build 다. 팬은 카메라만
    /// 움직이므로 같은 목록을 다시 쓰면 된다.
    /// (공간, tolerance, 배경, 흑백, **도면 판번호**) → 그릴 거리.
    ///
    /// revision 을 키에 넣는 게 편집과 캐시를 잇는 전부다. 엔진이 도면을 고칠
    /// 때마다 올려주므로, 선 하나만 그어도 다음 렌더에서 자동으로 다시 만든다.
    /// 이걸 빼면 그린 게 화면에 안 나타난다.
    cache: Option<(String, f64, u32, bool, u64, DisplayList)>,
    /// 깔린 밑그림들 — **푼 픽셀**. 비어 있으면 틀만 그려진다 (옛 동작).
    underlays: Vec<Underlay>,
}

/// 깔린 밑그림 한 장.
///
/// 네모(자리·크기)는 여기 안 둔다. 그건 도면 안의 엔티티가 들고 있고, 사람이
/// 끌어 옮기거나 지울 수 있다 — 복사해 두면 그릴 때마다 **옛 자리**에 그린다.
/// 그릴 때 `handle` 로 찾아서 지금 자리를 읽는다. 지워졌으면 못 찾으니 안
/// 그려진다, 따로 치울 필요 없이.
struct Underlay {
    handle: Handle,
    /// 알파가 **곱해진** RGBA (tiny-skia 규약).
    pm: tiny_skia::Pixmap,
    /// 도형의 불투명도. 밑그림은 흐리게 깔려서 그 위에 그은 선이 보인다.
    opacity: f32,
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
    /// 읽은 문·창 도형 수.
    openings: usize,
    /// 읽은 치수 도형 수.
    dims: usize,
    /// 읽은 공간 도형 수.
    zones: usize,
    /// 읽은 지시선 도형 수.
    leaders: usize,
    /// 읽은 물량표 도형 수.
    boqs: usize,
}

/// 그릴 거리를 만드는 옵션.
///
/// **투명도를 켠다.** 엔진은 기본으로 끈 채 그린다 (AutoCAD 의
/// TRANSPARENCYDISPLAY 를 따르는 자리다). 그러면 `Common::transparency` 가
/// 통째로 무시되고, Bimove 의 **공간(zone)이 불투명한 회색 덩어리**가 되어
/// 평면도를 덮는다 — 공간은 벽·가구 위에 비치게 깔리는 것이다
/// (`zone.rs`, `lib/zoneStyle.ts`).
///
/// DXF 로 연 도면에도 같이 걸린다. 그 쪽은 **더 맞는 그림**이 된다:
/// AutoCAD 도 TRANSPARENCYDISPLAY 기본이 켜짐이라, 투명하게 그려 둔 객체를
/// 지금까지는 불투명하게 보여주고 있었다.
/// `view_height` 는 **지금 화면에 들어오는 도면 높이**다 (세계 단위).
///
/// 이걸 안 주면 엔진이 "도면 전체 높이" 로 되돌아간다 — 그러면 PDSIZE 가 0 인
/// POINT(치수의 DEFPOINTS 가 보통 그렇다)가 **도면 높이의 5%** 로 구워진다.
/// 10만짜리 도면이면 X 하나가 5천 단위, 방만 하다. 줌을 해도 안 줄어든다 —
/// 세계 좌표로 구워졌기 때문이다.
///
/// AutoCAD 는 PDSIZE 가 0 이면 **화면** 높이의 5% 로 그리고 화면을 다시 그릴
/// 때마다 고쳐 잡는다. 화면 높이를 넘겨 주면 그와 같아진다 (늘 캔버스의 5%).
/// 밑그림 바이트를 **푼다.** 못 푸는 것은 세어서 `notes` 로 말한다.
///
/// 한 번만 한다 — 열 때. 그릴 때 풀면 팬 한 번에 수 MB 짜리 jpeg 디코딩이
/// 프레임마다 돈다.
fn decode_underlays(images: Vec<bimove::BimoveImage>, notes: &mut Vec<String>) -> Vec<Underlay> {
    let mut out = Vec::new();
    let mut failed = 0usize;
    for im in images {
        match decode_rgba(&im.bytes) {
            Some(pm) => out.push(Underlay { handle: im.handle, pm, opacity: im.opacity }),
            // 조용히 빼면 "밑그림이 날아갔다" 로 보인다. 틀은 그대로 서 있고,
            // 왜 그림이 안 나오는지는 끝말이 말해 준다.
            None => failed += 1,
        }
    }
    if failed > 0 {
        notes.push(format!("밑그림 {failed}장은 그림을 못 풀어서 틀만 그린다"));
    }
    out
}

/// png·jpeg·gif·webp → tiny-skia 픽스맵.
fn decode_rgba(bytes: &[u8]) -> Option<tiny_skia::Pixmap> {
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = (img.width(), img.height());
    let mut data = img.into_raw();
    // tiny-skia 는 **알파가 곱해진** 픽셀을 다룬다. 곱하지 않고 넣으면
    // 반투명한 자리가 하얗게 뜬다 (`demultiply_in_place` 의 반대 방향이다).
    for px in data.chunks_exact_mut(4) {
        let a = u32::from(px[3]);
        if a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            *c = ((u32::from(*c) * a + 127) / 255) as u8;
        }
    }
    tiny_skia::Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)
}

/// 밑그림 한 장을 화면에 얹는다. 지워졌거나 자리가 망가졌으면 아무것도 안 한다.
///
/// ── 셈 ──
///
/// 그림 픽셀 `(px, py)` (왼쪽 **위**가 0, y 는 아래로) → 세상 좌표:
///
/// ```text
/// world = o + (px / W) * U + (1 - py / H) * V
/// ```
///
/// `o` 는 엔티티의 `insert` (왼쪽 **아래**), `U`·`V` 는 가로·세로 변 벡터
/// (`u * size.x`, `v * size.y` — `render` 크레이트가 틀을 그릴 때 쓰는 것과
/// 같은 식이다). 거기에 화면 변환(배율 + y 뒤집기 + 밀기)을 먹이면 아핀 하나가
/// 나온다. 돌아간 그림도 이 식 그대로 든다.
fn blit_underlay(pm: &mut tiny_skia::Pixmap, u: &Underlay, d: &cadcraft_doc::Drawing, view: &cadcraft_render::raster::View) {
    let Some(e) = d.model.iter().find(|e| e.handle == u.handle) else { return };
    let EntityKind::Image(i) = &e.kind else { return };
    let (w, h) = (u.pm.width() as f64, u.pm.height() as f64);
    if w < 1.0 || h < 1.0 {
        return;
    }
    let o = i.insert.xy();
    let uu = i.u.xy() * i.size.x;
    let vv = i.v.xy() * i.size.y;
    // 화면 변환의 **선형 부분**. 밀기는 아래 `to_screen` 이 한 번에 먹인다.
    let lin = |v: cadcraft_geom::Vec2| (v.x * view.scale, -v.y * view.scale);
    let (ax, ay) = lin(uu / w);
    let (bx, by) = lin(vv / -h);
    let (tx, ty) = view.to_screen(o + vv);
    let t = tiny_skia::Transform::from_row(ax as f32, ay as f32, bx as f32, by as f32, tx, ty);
    if !t.is_finite() {
        return;
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: u.opacity.clamp(0.0, 1.0),
        // 밑그림은 거의 늘 줄여서 깔린다 (`placeImage` 가 화면에 맞춘다).
        // 최근접으로 줄이면 가는 선이 통째로 사라져서 — 도면 밑그림에서는
        // 그게 **그려야 할 벽**이다 — 쌍선형으로 둔다.
        quality: tiny_skia::FilterQuality::Bilinear,
        ..Default::default()
    };
    pm.draw_pixmap(0, 0, u.pm.as_ref(), &paint, t, None);
}

fn render_opts(tolerance: f64, view_height: f64) -> Options {
    Options { tolerance, transparency: true, view_height, ..Options::default() }
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
        Ok(Doc::of(drawing, Vec::new(), Vec::new(), Counts::default(), String::new(), Vec::new()))
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
            openings: load.opening_count,
            dims: load.dim_count,
            zones: load.zone_count,
            boqs: load.boq_count,
            leaders: load.leader_count,
        };
        Ok(Doc::of(load.drawing, load.pages, load.notes, counts, load.page_id, load.images))
    }

    fn of(
        drawing: cadcraft_doc::Drawing, pages: Vec<(String, String)>, notes: Vec<String>,
        counts: Counts, page_id: String, images: Vec<bimove::BimoveImage>,
    ) -> Doc {
        let mut notes = notes;
        let underlays = decode_underlays(images, &mut notes);
        // 심긴 id 는 **세션에 넣기 전에** 뜬다 — 넣은 뒤에는 도면을 빌려야
        // 하고, 그 사이에 편집이 끼면 기준이 "열 때" 가 아니게 된다.
        let loaded_ids = to_bimove::ids_in(&drawing);
        let mut session = Session::empty();
        session.open_drawing(drawing, "bimove", None);
        Doc { session, pages, notes, counts, page_id, loaded_ids, cache: None, underlays }
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
    /// 지금 도면을 DXF 글로 쓴다.
    ///
    /// 엔진이 문서를 들기로 한 이상 **저장도 엔진에서 나와야 한다.** 이게
    /// 없으면 고친 것이 창을 닫는 순간 사라진다 — 그리기·되돌리기가 다 돌아도
    /// 쓸 수가 없다.
    fn to_dxf(&self) -> String {
        cadcraft_io::write_dxf(self.drawing())
    }

    /// 지금 도면을 **Bimove 저장본으로** 되돌린다.
    ///
    /// `original_json` 은 이 Doc 을 연 그 문서다. 엔진이 모르는 것을 통과시키려면
    /// 원본이 있어야 한다 (`to_bimove` 모듈 주석) — 들고 있지 않고 받는 이유는
    /// 큰 문서를 두 벌 쥐지 않기 위해서다. JS 쪽에 이미 있다.
    fn to_bimove(&self, original_json: &str) -> Result<to_bimove::SaveBack, String> {
        to_bimove::drawing_to_bimove(
            original_json,
            &self.page_id,
            &self.loaded_ids,
            self.drawing(),
        )
    }

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

    /// 읽은 문·창 **도형** 수.
    ///
    /// 하나가 엔티티 여럿이 된다 (벽을 가리는 사각형 + 잼브 둘 + 문짝 + 호).
    /// 그래서 엔티티 수로는 몇 개가 들어왔는지 되짚을 수 없다.
    #[wasm_bindgen(js_name = openingCount)]
    pub fn opening_count(&self) -> usize {
        self.0.counts.openings
    }

    /// 읽은 치수 **도형** 수.
    ///
    /// 숫자가 맞는지는 이것만 봐서는 모른다 — 축척을 문서에서 못 찾으면
    /// `notes` 에 그 얘기가 올라오므로 **둘을 같이** 봐야 한다.
    #[wasm_bindgen(js_name = dimCount)]
    pub fn dim_count(&self) -> usize {
        self.0.counts.dims
    }

    /// 읽은 공간 **도형** 수.
    ///
    /// 하나가 엔티티 넷이 된다 (면 + 테두리 + 이름 + 면적).
    #[wasm_bindgen(js_name = zoneCount)]
    pub fn zone_count(&self) -> usize {
        self.0.counts.zones
    }

    /// 읽은 물량표 **도형** 수.
    ///
    /// 하나가 엔티티 하나다 (TABLE). 칸과 괘선은 엔진이 그 안에서 만든다.
    #[wasm_bindgen(js_name = boqCount)]
    pub fn boq_count(&self) -> usize {
        self.0.counts.boqs
    }

    /// 읽은 지시선 **도형** 수 (tldraw `arrow`).
    #[wasm_bindgen(js_name = leaderCount)]
    pub fn leader_count(&self) -> usize {
        self.0.counts.leaders
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

    /// 지금 도면을 DXF 글로. 저장할 때 이걸 올린다.
    #[wasm_bindgen(js_name = toDxf)]
    pub fn to_dxf(&self) -> String {
        self.0.to_dxf()
    }

    /// 지금 도면을 **Bimove 저장본 JSON** 으로 되돌린다.
    ///
    /// `originalJson` 은 이 도면을 연 그 문서를 그대로 넘긴다. 엔진이 모르는
    /// 것(밑그림 image, 다른 페이지, tldraw 도형)은 거기서 통과한다 — 안 넘기면
    /// 새로 쓰는 셈이 되어 그게 다 날아간다.
    ///
    /// 돌려주는 것은 `{ json, notes, kept, erased, added }` 를 담은 JSON 글이다.
    /// `notes` 가 비어 있지 않으면 **되읽지 못한 것이 있다** — 화면에 올려야
    /// 한다. 조용히 넘기면 그린 게 사라진 걸 아무도 모른다.
    #[wasm_bindgen(js_name = toBimove)]
    pub fn to_bimove(&self, original_json: &str) -> Result<String, JsError> {
        let back = self.0.to_bimove(original_json).map_err(|e| JsError::new(&e))?;
        Ok(serde_json::json!({
            "json": back.json,
            "notes": back.notes,
            "kept": back.kept,
            "erased": back.erased,
            "added": back.added,
        })
        .to_string())
    }

    /// 한 점을 찍어 엔티티를 고른다. `aperture` 는 **도면 단위** (픽셀 ÷ 줌).
    pub fn pick(&self, layout: &str, x: f64, y: f64, aperture: f64) -> Option<String> {
        self.0.pick(layout, x, y, aperture)
    }

    /// 이 엔티티와 같은 Bimove 객체(문·창·벽·공간…)인 손잡이들. 없으면 빈 배열.
    ///
    /// `pick` 으로 하나를 집은 다음 이걸로 묶음을 펼친다. 자세한 건
    /// `Doc::group` 주석에.
    pub fn group(&self, handle: &str) -> Vec<String> {
        self.0.group(handle)
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
    /// `grid` 면 **바탕 바로 위에** 격자를 깐다 (도면보다 아래). 칸 간격은
    /// 배율을 보고 1·2·5 사다리에서 고른다 (`grid.rs`).
    /// `gray` 면 색을 **먹 농도**로 바꾼다 — 선·글자·해치 세 단 (`to_gray`).
    #[wasm_bindgen(js_name = renderPixels)]
    #[allow(clippy::too_many_arguments)]
    pub fn render_pixels(
        &mut self, layout: &str, cx: f64, cy: f64, scale: f64, w: u32, h: u32, bg: u32,
        draft: bool, grid: bool, gray: bool,
    ) -> Option<BimoveRaster> {
        self.0.raster(layout, cx, cy, scale, w, h, bg, draft, grid, gray).map(BimoveRaster)
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

    /// **색이 있는** 저장본 — 흑백 보기가 정말 먹을 바꾸는지 보려면 색이
    /// 있어야 한다. 공간을 쓴다: 이름 글자 색이 `props.color` 로 들어간다
    /// (벽은 제 색이 없어서 회색만 나온다 — 그걸로 쟀다가 한 번 틀렸다).
    fn colored_line_doc() -> String {
        serde_json::json!({
            "pages": [{ "id": "page:1", "name": "1" }],
            "scale": { "pxPerMm": 1.0 },
            "shapes": [{
                "id": "shape:z1", "type": "zone", "x": 0.0, "y": 0.0, "rotation": 0.0,
                "index": "a1", "parentId": "page:1",
                "props": {
                    "points": [
                        { "x": 0.0, "y": 0.0 }, { "x": 2000.0, "y": 0.0 },
                        { "x": 2000.0, "y": 1000.0 }, { "x": 0.0, "y": 1000.0 },
                    ],
                    "label": "거실", "areaM2": 2.0, "wallHeightMm": 2400,
                    "color": "#e01b24",
                },
            }],
            "records": {},
            "source": { "from": "tldraw", "at": 0, "shapeCount": 1 },
        })
        .to_string()
    }

    /// 화면에 찍힌 색 가운데 **바탕이 아닌 것**들.
    fn inks(r: &Raster, bg: (u8, u8, u8)) -> Vec<(u8, u8, u8)> {
        let mut out: Vec<(u8, u8, u8)> = r
            .rgba
            .chunks_exact(4)
            .filter(|p| p[3] > 200)
            .map(|p| (p[0], p[1], p[2]))
            .filter(|c| *c != bg)
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    #[test]
    fn 흑백_보기는_색을_먹으로_바꾼다() {
        // 한: "여기 gray on off 원래 기능 있었는데 없네. 그거 넣자"
        let mut doc = Doc::from_bimove(&colored_line_doc(), "").unwrap();
        let (w, h) = (200, 100);
        let color = doc.raster("", 1000.0, -500.0, 0.09, w, h, 0xFFFFFF, false, false, false).unwrap();
        let gray = doc.raster("", 1000.0, -500.0, 0.09, w, h, 0xFFFFFF, false, false, true).unwrap();

        let c = inks(&color, (255, 255, 255));
        let g = inks(&gray, (255, 255, 255));
        assert!(!c.is_empty() && !g.is_empty(), "아무것도 안 그려졌다");
        // 색 쪽에는 **치우친 색**이 있다 (빨강).
        assert!(c.iter().any(|(r, gg, b)| r.abs_diff(*gg) > 30 || gg.abs_diff(*b) > 30),
            "색 보기인데 치우친 색이 없다: {c:?}");
        // 흑백 쪽은 **전부 무채색**이다 — 세 값이 거의 같다. (가장자리
        // 안티앨리어싱은 바탕과 섞이므로 역시 무채색이다.)
        for (r, gg, b) in &g {
            assert!(
                r.abs_diff(*gg) <= 2 && gg.abs_diff(*b) <= 2,
                "흑백인데 색이 남았다: {:?}",
                (r, gg, b)
            );
        }
    }

    #[test]
    fn 흑백에서도_흰_면은_희게_남는다() {
        // **처음엔 면도 정해진 해치 먹으로 칠했다.** 그랬더니 벽이 통째로
        // 회색 덩어리가 됐다 — 벽 속을 채우는 흰 면까지 같이 칠해져서다.
        // 그 면은 해치가 아니라 바닥이고, 흰 것이 목적이다.
        let mut doc = Doc::from_bimove(&wall_doc(), "").unwrap();
        let (w, h) = (200, 100);
        let r = doc.raster("", 1000.0, 0.0, 0.08, w, h, 0xFFFFFF, false, false, true).unwrap();
        // 벽 **속**은 흰색이어야 한다 — 가운데 한 점을 본다.
        let i = ((h / 2) as usize * w as usize + (w / 2) as usize) * 4;
        let mid = (r.rgba[i], r.rgba[i + 1], r.rgba[i + 2]);
        assert_eq!(mid, (255, 255, 255), "흑백에서 벽 속이 회색으로 메워졌다: {mid:?}");
    }

    /// 벽 한 줄 — 속이 흰 면이 들어 있다.
    fn wall_doc() -> String {
        serde_json::json!({
            "pages": [{ "id": "page:1", "name": "1" }],
            "scale": { "pxPerMm": 1.0 },
            "shapes": [{
                "id": "shape:w1", "type": "wall", "x": 0.0, "y": 0.0, "rotation": 0.0,
                "index": "a1", "parentId": "page:1",
                "props": { "x2": 2000.0, "y2": 0.0, "thickness": 600.0 },
            }],
            "records": {},
            "source": { "from": "tldraw", "at": 0, "shapeCount": 1 },
        })
        .to_string()
    }

    #[test]
    fn 흑백을_껐다_켜면_다시_그린다() {
        // 캐시 키에 흑백이 없으면 토글을 눌러도 쓰던 목록을 그대로 써서
        // **아무 일도 안 난다.** 그게 이 시험이 막는 자리다.
        let mut doc = Doc::from_bimove(&colored_line_doc(), "").unwrap();
        let (w, h) = (200, 100);
        let a = doc.raster("", 1000.0, -500.0, 0.09, w, h, 0xFFFFFF, false, false, false).unwrap();
        let b = doc.raster("", 1000.0, -500.0, 0.09, w, h, 0xFFFFFF, false, false, true).unwrap();
        let c = doc.raster("", 1000.0, -500.0, 0.09, w, h, 0xFFFFFF, false, false, false).unwrap();
        assert_ne!(a.rgba, b.rgba, "흑백을 켰는데 그림이 그대로다");
        assert_eq!(a.rgba, c.rgba, "흑백을 껐는데 색이 안 돌아왔다");
    }

    #[test]
    fn 어두운_바탕에서는_밝은_먹을_쓴다() {
        let mut doc = Doc::from_bimove(&colored_line_doc(), "").unwrap();
        let (w, h) = (200, 100);
        let bg = (0x1e, 0x1e, 0x22);
        let r = doc.raster("", 1000.0, -500.0, 0.09, w, h, 0x1E1E22, false, false, true).unwrap();
        let g = inks(&r, bg);
        assert!(!g.is_empty(), "아무것도 안 그려졌다");
        // 바탕(명도 ~30)보다 **밝아야** 보인다.
        let brightest = g.iter().map(|(r, _, _)| *r).max().unwrap_or(0);
        assert!(brightest > 120, "어두운 바탕인데 먹이 어둡다 — 가장 밝은 값 {brightest}");
    }

    #[test]
    fn 픽셀로_그리면_선이_실제로_찍힌다() {
        // 크기만 맞고 전부 배경색이면 "그려진 척" 이다. 배경이 아닌 픽셀이
        // 있어야 진짜 그린 것이다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let (w, h) = (200, 100);
        // (0,0)~(100,50) 선을 가운데(50,25)에 두고 1.5px/단위로
        let r = doc.raster("", 50.0, 25.0, 1.5, w, h, 0xFFFFFF, false, false, false).expect("래스터 실패");
        assert_eq!(r.w, w);
        assert_eq!(r.h, h);
        assert_eq!(r.rgba.len(), (w as usize) * (h as usize) * 4, "RGBA 길이가 안 맞는다");

        let bg = r.rgba.chunks_exact(4).filter(|p| p[0] == 255 && p[1] == 255 && p[2] == 255).count();
        let ink = (w as usize) * (h as usize) - bg;
        assert!(ink > 20, "배경이 아닌 픽셀이 {ink}개뿐 — 선이 안 찍혔다");
        assert!(bg > 0, "전부 잉크다 — 배경색이 안 먹었다");
    }

    /// 2x2 불투명 png — **왼쪽 위부터** 빨강·초록 / 파랑·검정.
    ///
    /// 한 칸짜리로는 **위아래가 뒤집혔는지**를 못 본다. 저장본의 y 는 아래로
    /// 가고 엔진의 y 는 위로 가서, 그 뒤집기가 이 블릿에서 가장 틀리기 쉬운
    /// 자리다. 네 칸이면 어느 모서리가 어디로 갔는지가 그대로 보인다.
    const QUAD_PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAEklEQVR4nGP4z8DwHwyBNJgAAEDRBvrma+v4AAAAAElFTkSuQmCC";

    /// 밑그림 한 장만 깔린 저장본. 캔버스 (100,200) 에 400x300.
    /// 엔진 좌표로는 x 100..500, y -500..-200 이다.
    fn underlay_json(opacity: f64) -> String {
        underlay_with(opacity, "")
    }

    fn underlay_with(opacity: f64, extra: &str) -> String {
        format!(
            r#"{{"version":1,"pages":[{{"id":"page:main","name":"모형","index":"a1"}}],
               "shapes":[{{"id":"shape:bg","type":"image","x":100,"y":200,"rotation":0,
                 "index":"a1","parentId":"page:main","opacity":{opacity},
                 "props":{{"w":400,"h":300,"assetId":"asset:a1"}}}}{extra}],
               "records":{{"asset:a1":{{"id":"asset:a1","typeName":"asset","type":"image",
                 "props":{{"src":"{QUAD_PNG}","w":2,"h":2}}}}}},"skipped":0,
               "source":{{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":1}}}}"#
        )
    }

    /// 화면 비율 자리 `(fx, fy)` 픽셀의 RGB. `fy` 는 **위에서부터**.
    fn at(r: &Raster, fx: f64, fy: f64) -> (u8, u8, u8) {
        let x = (f64::from(r.w) * fx) as usize;
        let y = (f64::from(r.h) * fy) as usize;
        let i = (y * r.w as usize + x) * 4;
        (r.rgba[i], r.rgba[i + 1], r.rgba[i + 2])
    }
    fn mid_rgb(r: &Raster) -> (u8, u8, u8) {
        at(r, 0.5, 0.5)
    }

    #[test]
    fn 밑그림이_진짜_픽셀로_깔린다() {
        // 전엔 네모 틀과 대각선 둘만 그렸다 (AutoCAD 가 그림을 못 찾았을 때의
        // 모양). 자리는 맞지만 **그 위에 따라 그릴 수가 없다** — 벽이 어디
        // 있는지 안 보이니까.
        let mut doc = Doc::from_bimove(&underlay_json(1.0), "").unwrap();
        // 그림이 화면을 꽉 채우게 둔다: 가운데 (300, -350), 1px = 1단위.
        let r = doc.raster("", 300.0, -350.0, 1.0, 400, 300, 0xFFFFFF, false, false, false).expect("래스터 실패");
        // **네 모서리가 제자리에 있어야 한다.** 위아래가 뒤집히면 여기서
        // 빨강과 파랑이 자리를 바꾼다 — 밑그림에서 그건 도면이 거꾸로
        // 깔렸다는 뜻이고, 그 위에 그린 벽이 전부 틀린 자리에 선다.
        // 딱 떨어지는 값을 재지 않는다 — 쌍선형으로 늘리므로 이웃 칸이 한두
        // 단계 배어 나온다. 뒤집힘은 빨강↔파랑처럼 **크게** 어긋나므로 이
        // 느슨함으로도 다 잡힌다.
        let near = |got: (u8, u8, u8), want: (u8, u8, u8), what: &str| {
            let d = |a: u8, b: u8| i32::from(a) - i32::from(b);
            assert!(
                d(got.0, want.0).abs() < 24 && d(got.1, want.1).abs() < 24 && d(got.2, want.2).abs() < 24,
                "{what}: {got:?} (바란 것 {want:?})",
            );
        };
        near(at(&r, 0.25, 0.25), (255, 0, 0), "왼쪽 위가 빨강이 아니다");
        near(at(&r, 0.75, 0.25), (0, 255, 0), "오른쪽 위가 초록이 아니다");
        near(at(&r, 0.25, 0.75), (0, 0, 255), "왼쪽 아래가 파랑이 아니다");
        near(at(&r, 0.75, 0.75), (0, 0, 0), "오른쪽 아래가 검정이 아니다");
    }

    #[test]
    fn 그린_선이_밑그림_위에_보인다() {
        // **순서가 전부다.** 그림을 나중에 얹으면 그 위에 그은 선을 덮어서,
        // 밑그림을 따라 그리는 일 자체가 안 된다 — 그은 선이 안 보이니까.
        //
        // 불투명한 그림 **위 절반**(빨강·초록)에 자홍색 선을 긋는다. 네 칸
        // 어디에도 자홍은 없고, 빨강과 초록을 섞어도 자홍이 안 나온다 —
        // 그러니 자홍 픽셀이 보이면 선이 그림 위에 그어진 것이다.
        //
        // `r##` 다 — 색이 `"#ff00ff"` 라 `"#` 가 `r#"…"#` 를 거기서 닫는다.
        let line = r##",{"id":"shape:l","type":"dxfgroup","x":150,"y":275,"rotation":0,
            "index":"a2","parentId":"page:main",
            "meta":{"dxfColor":"#ff00ff"},"props":{"pathData":"M0,0L300,0"}}"##;
        let shot = |extra: &str| {
            let mut doc = Doc::from_bimove(&underlay_with(1.0, extra), "").unwrap();
            doc.raster("", 300.0, -350.0, 1.0, 400, 300, 0xFFFFFF, false, false, false).expect("래스터 실패").rgba
        };
        let (bare, drawn) = (shot(""), shot(line));
        // 1px 짜리 선이라 **딱 그 색**은 안 나온다 (계단 다듬기가 섞는다).
        // 그러니 "그림만" 과 견준다 — 선이 그림 위에 그어졌으면 그 줄의
        // 픽셀이 달라지고, 밑에 깔렸으면 한 픽셀도 안 달라진다.
        let moved = bare.chunks_exact(4).zip(drawn.chunks_exact(4)).filter(|(a, b)| a != b).count();
        assert!(moved > 100, "달라진 픽셀이 {moved}개 — 그림이 그은 선을 덮었다");
    }

    #[test]
    fn 밑그림은_흐리게_깔린다() {
        //
        // 흐리게 깐 밑그림(불투명도 0.4)은 흰 바탕과 섞인다. 왼쪽 위 칸은
        // 빨강이니 **연한 빨강**이 나와야 한다 — 진한 빨강이면 불투명도를
        // 안 받은 것이고, 흰색이면 아예 안 그려진 것이다.
        let mut doc = Doc::from_bimove(&underlay_json(0.4), "").unwrap();
        let r = doc.raster("", 300.0, -350.0, 1.0, 400, 300, 0xFFFFFF, false, false, false).expect("래스터 실패");
        let (rr, gg, bb) = at(&r, 0.25, 0.25);
        assert!(rr > 200, "빨강 기운이 없다: {rr},{gg},{bb}");
        assert!(gg > 120 && gg < 200, "흰 바탕과 안 섞였다: {rr},{gg},{bb}");
        assert_eq!(gg, bb, "초록과 파랑이 다르다 — 섞는 셈이 틀렸다: {rr},{gg},{bb}");
    }

    #[test]
    fn 밑그림을_지우면_같이_사라진다() {
        // 픽셀을 엔티티와 따로 들고 있으니, 엔티티가 없어졌는데 그림만 남는
        // 일이 생길 수 있다. 자리도 엔티티에서 읽는다 — 그래서 지우면 찾을
        // 것이 없어 안 그려진다.
        let mut doc = Doc::from_bimove(&underlay_json(1.0), "").unwrap();
        let h = doc
            .drawing()
            .model
            .iter()
            .find(|e| matches!(e.kind, EntityKind::Image(_)))
            .map(|e| e.handle.hex())
            .expect("밑그림이 없다");
        doc.exec("erase", &format!(r#"{{"handles":["{h}"]}}"#)).unwrap();
        let r = doc.raster("", 300.0, -350.0, 1.0, 400, 300, 0xFFFFFF, false, false, false).expect("래스터 실패");
        let (rr, gg, bb) = mid_rgb(&r);
        assert_eq!((rr, gg, bb), (255, 255, 255), "지웠는데 그림이 남았다: {rr},{gg},{bb}");
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
        doc.raster("", 0.0, 0.0, 0.001, 64, 64, 0xFFFFFF, false, false, false);
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
        let r = doc.raster("", 50.0, -25.0, 1.5, 200, 100, 0xFFFFFF, false, false, false).unwrap();
        let ink = r.rgba.chunks_exact(4).filter(|p| !(p[0] == 255 && p[1] == 255 && p[2] == 255)).count();
        assert!(ink > 20, "덜어내고 나니 {ink} 픽셀뿐 — 도면이 사라졌다");
    }


    // ── 점 크기는 화면을 따라간다 ──

    /// 선 둘로 크기를 정한 도면 + 가운데 POINT 하나짜리 ASCII DXF.
    ///
    /// `$PDSIZE` 가 0 이라 점은 **상대 크기**로 그려진다 — 무엇에 상대인지가
    /// 이 테스트가 보는 것이다.
    fn dxf_with_point(span: f64) -> Vec<u8> {
        let mut v: Vec<String> = Vec::new();
        let mut g = |code: &str, val: String| { v.push(code.into()); v.push(val) };
        g("0", "SECTION".into()); g("2", "HEADER".into());
        g("9", "$PDMODE".into()); g("70", "3".into());   // X 자
        g("9", "$PDSIZE".into()); g("40", "0".into());   // 상대 크기
        g("0", "ENDSEC".into());
        g("0", "SECTION".into()); g("2", "ENTITIES".into());
        g("0", "LINE".into()); g("8", "0".into());
        g("10", "0".into()); g("20", "0".into());
        g("11", format!("{span}")); g("21", "0".into());
        g("0", "LINE".into()); g("8", "0".into());
        g("10", "0".into()); g("20", "0".into());
        g("11", "0".into()); g("21", format!("{}", span / 5.0));
        g("0", "POINT".into()); g("8", "0".into());
        g("10", format!("{}", span / 2.0)); g("20", format!("{}", span / 10.0));
        g("0", "ENDSEC".into()); g("0", "EOF".into());
        (v.join("\r\n") + "\r\n").into_bytes()
    }

    /// 창 안 잉크의 가로 폭(px). 선은 멀리 있어 안 들어온다 — 점만 잡힌다.
    fn ink_width(doc: &mut Doc, cx: f64, cy: f64, scale: f64, w: u32, h: u32) -> u32 {
        let r = doc.raster("", cx, cy, scale, w, h, 0xFFFFFF, false, false, false).expect("그려야 한다");
        let px = &r.rgba;
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        for i in (0..px.len()).step_by(4) {
            if px[i] == 255 && px[i + 1] == 255 && px[i + 2] == 255 { continue }
            let x = ((i / 4) as u32) % w;
            lo = lo.min(x);
            hi = hi.max(x);
        }
        if lo == u32::MAX { 0 } else { hi - lo + 1 }
    }

    #[test]
    fn 점은_줌을_해도_화면에서_같은_크기다() {
        // 전엔 `view_height` 를 안 넘겨서 엔진이 **도면 전체 높이**로 되돌아갔다.
        // 그러면 점이 도면 높이의 5% 로 **세계 좌표에 구워져** 10만짜리 도면에서
        // X 하나가 5천 단위, 방만 해진다. 줌을 해도 안 줄어든다.
        let span = 100_000.0;
        let mut doc = Doc::open(&dxf_with_point(span)).expect("DXF 를 읽어야 한다");
        let (at_x, at_y) = (span / 2.0, span / 10.0);
        let (w, h) = (400u32, 400u32);

        // 두 배율에서 재면 **같은 픽셀 수**가 나와야 한다 (화면의 5%).
        let half = span / 40.0;
        let s1 = f64::from(w) / 2.0 / half;
        let a = ink_width(&mut doc, at_x, at_y, s1, w, h);
        let b = ink_width(&mut doc, at_x, at_y, s1 * 4.0, w, h);

        assert!(a > 0 && b > 0, "점이 안 그려졌다 ({a}px, {b}px)");
        // 화면 높이의 5% 언저리. 창을 꽉 채우면 세계 좌표에 구워진 것이다.
        assert!(
            (10..=60).contains(&a) && (10..=60).contains(&b),
            "점이 화면의 5% 가 아니다 — {a}px, {b}px (창 {h}px)",
        );
        let (lo, hi) = (a.min(b), a.max(b));
        assert!(
            hi <= lo + lo / 4 + 2,
            "줌에 따라 점 크기가 달라진다 — {a}px vs {b}px",
        );
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
             "parentId":"page:main","props":{"x2":100,"y2":0,"thickness":20}},
            {"id":"shape:d","type":"door","x":50,"y":60,"rotation":0,"index":"a3",
             "parentId":"page:main","props":{"width":40,"thickness":20,"swing":1,"flipped":false}},
            {"id":"shape:m","type":"dimension","x":0,"y":60,"rotation":0,"index":"a4",
             "parentId":"page:main","props":{"x2":100,"y2":0,"offset":40}},
            {"id":"shape:z","type":"zone","x":0,"y":0,"rotation":0,"index":"a5",
             "parentId":"page:main",
             "props":{"points":[{"x":0,"y":0},{"x":100,"y":0},{"x":100,"y":60},{"x":0,"y":60}],
                      "label":"거실","areaM2":6,"perimeterM":0,
                      "wallHeightMm":2400,"color":"#34a853"}},
            {"id":"shape:t","type":"boqtable","x":200,"y":0,"rotation":0,"index":"a6",
             "parentId":"page:main",
             "props":{"w":612,"h":96,"title":"마감재 물량 산출표",
                      "rowsJson":"[{\"category\":\"벽체\",\"item\":\"석고보드\",\"spec\":\"9.5T\",\"areaM2\":6,\"qty\":4,\"unit\":\"장\"}]",
                      "totalAreaM2":6}}],
          "records":{},"skipped":0,
          "source":{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":6}}"##
            .to_string()
    }

    #[test]
    fn bimove_문서를_열어_픽셀로_그린다() {
        // 여기까지 통하면 렌더·선택·스냅·편집이 전부 같은 길로 붙는다.
        // 배경색뿐이면 "그려진 척" 이므로 잉크를 센다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let (w, h) = (200, 100);
        // 캔버스 y 가 뒤집히므로 도면은 y = -50..0 에 있다
        let r = doc.raster("", 50.0, -25.0, 1.5, w, h, 0xFFFFFF, false, false, false).expect("래스터 실패");
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
        assert!(!notes.contains("dimension"), "치수는 그린다: {notes}");
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
    fn bimove_문서의_문이_벽_뒤에_들어간다() {
        // 문은 벽을 **가려서** 갭을 뚫는다. 그리는 차례에 기대는 구조라
        // 벽보다 먼저 들어가면 갭이 벽에 덮여 사라진다.
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        assert_eq!(doc.counts.openings, 1);
        let at = |f: fn(&cadcraft_doc::EntityKind) -> bool| {
            doc.drawing().model.iter().position(|e| f(&e.kind))
        };
        let mline = at(|k| matches!(k, cadcraft_doc::EntityKind::MLine(_))).expect("벽이 없다");
        let gap = at(|k| matches!(k, cadcraft_doc::EntityKind::Wipeout(_))).expect("가림이 없다");
        assert!(mline < gap, "벽 {mline} / 가림 {gap}");
    }

    #[test]
    fn bimove_문서의_공간이_맨_마지막에_들어간다() {
        // 공간은 반투명이라 밑에 깔린 것이 결과에 들어온다. 먼저 들어가면
        // 벽이 공간 위에 덮여서 비치는 효과가 사라진다.
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        assert_eq!(doc.counts.zones, 1);
        let at = |f: fn(&cadcraft_doc::EntityKind) -> bool| {
            doc.drawing().model.iter().position(|e| f(&e.kind))
        };
        let mline = at(|k| matches!(k, cadcraft_doc::EntityKind::MLine(_))).expect("벽이 없다");
        let fill = at(|k| matches!(k, cadcraft_doc::EntityKind::Hatch(_))).expect("공간 면이 없다");
        assert!(mline < fill, "벽 {mline} / 공간 {fill}");
    }

    #[test]
    fn bimove_문서의_물량표가_table_로_들어간다() {
        // 선·글자로 풀면 그림만 같고 표가 아니게 된다 — 칸을 집을 수도,
        // DXF 로 내보내 다른 CAD 에서 표로 읽을 수도 없다.
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        assert_eq!(doc.counts.boqs, 1);
        let t = doc
            .drawing()
            .model
            .iter()
            .find_map(|e| match &e.kind {
                cadcraft_doc::EntityKind::Table(t) => Some(t.clone()),
                _ => None,
            })
            .expect("표가 없다");
        assert_eq!(t.col_widths.len(), 6);
        // 제목 + 머리 + 본문 1 + 합계
        assert_eq!(t.row_heights.len(), 4);
        assert_eq!(t.cells[2][1].text, "석고보드");
    }

    #[test]
    fn bimove_문서의_치수가_dimension_으로_들어간다() {
        // 선과 글자를 따로 넣으면 그 순간 숫자가 굳는다. DIMENSION 이면
        // 두 점만 들고 있고 길이는 렌더 때 다시 잰다.
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        assert_eq!(doc.counts.dims, 1);
        let d = doc
            .drawing()
            .model
            .iter()
            .find_map(|e| match &e.kind {
                cadcraft_doc::EntityKind::Dimension(d) => Some(d.clone()),
                _ => None,
            })
            .expect("치수가 없다");
        assert!(d.text.is_empty(), "글자를 박아 뒀다: {:?}", d.text);
        // 축척이 문서에 없으므로 그 얘기가 올라와 있어야 한다
        assert!(doc.notes.iter().any(|n| n.contains("축척")), "{:?}", doc.notes);
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
            let r = d.raster("", 50.0, -50.0, 1.8, w as u32, h as u32, 0xFFFFFF, false, false, false).unwrap();
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
        let r = doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false, false, false).unwrap();
        assert!(r.rgba.chunks_exact(4).all(|p| p[3] == 255), "알파가 255 가 아닌 픽셀이 있다");
    }

    #[test]
    fn 팬은_목록을_다시_만들지_않는다() {
        // 카메라만 움직이면 같은 DisplayList 를 쓴다. 이게 깨지면 45MB 도면에서
        // 한 프레임이 15ms 에서 240ms 로 돌아간다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false, false, false).unwrap();
        let built = doc.cache.as_ref().map(|(_, t, _, _, _, dl)| (*t, dl.prims.len())).unwrap();
        doc.raster("", 999.0, -999.0, 1.5, 64, 64, 0xFFFFFF, false, false, false).unwrap();
        let after = doc.cache.as_ref().map(|(_, t, _, _, _, dl)| (*t, dl.prims.len())).unwrap();
        assert_eq!(built, after, "팬만 했는데 목록이 다시 만들어졌다");
    }

    #[test]
    fn 줌이_크게_바뀌면_다시_만든다() {
        // tolerance 는 줌에 반비례한다. 2배 안쪽은 그대로 쓰고, 넘으면 다시
        // 만들어야 곡선이 거칠어지지 않는다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.0, 64, 64, 0xFFFFFF, false, false, false).unwrap();
        let t0 = doc.cache.as_ref().unwrap().1;

        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false, false, false).unwrap();
        assert_eq!(doc.cache.as_ref().unwrap().1, t0, "1.5배 줌은 다시 만들 일이 아니다");

        doc.raster("", 50.0, 25.0, 10.0, 64, 64, 0xFFFFFF, false, false, false).unwrap();
        assert_ne!(doc.cache.as_ref().unwrap().1, t0, "10배 줌인데 그대로 썼다");
    }

    #[test]
    fn 공간이_바뀌면_다시_만든다() {
        // 모형 ↔ 배치는 아예 다른 내용이다. 캐시를 그대로 쓰면 엉뚱한 걸 그린다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false, false, false).unwrap();
        assert_eq!(doc.cache.as_ref().unwrap().0, "");
        doc.raster("배치1", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF, false, false, false).unwrap();
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

/// 삼각형 하나짜리 DisplayList. `h` 가 가리키는 엔티티 종류로 갈린다.
    fn tris_of(h: cadcraft_doc::Handle, color: Rgb) -> DisplayList {
        let mut dl = DisplayList::default();
        dl.prims.push(cadcraft_render::DPrim {
            handle: h,
            color,
            aci7: false,
            lw: 0.0,
            transparency: 0,
            kind: Kind::Tris,
            start: 0,
            len: 0,
        });
        dl
    }

    #[test]
    fn 어두운_바탕에서_글자_삼각형은_밝아진다() {
        // 글자 글리프는 외곽선을 쪼갠 **삼각형**이다. 면과 한 덩이로
        // 건너뛰면 어두운 캔버스에서 지시선은 밝아지는데 그 끝에 붙은
        // 글만 묻힌다 — 실제로 그렇게 한 번 나왔다 (`darkgaps.mjs`).
        let mut d = cadcraft_doc::Drawing::new_metric();
        let h = d.new_handle();
        d.model.push(cadcraft_doc::Entity::new(
            h,
            EntityKind::MText(cadcraft_doc::MText {
                insert: cadcraft_geom::Vec3::ZERO,
                height: 20.0,
                width: 0.0,
                attach: 1,
                rotation: 0.0,
                style: "Standard".into(),
                contents: "여기".into(),
                line_spacing: 1.0,
                line_spacing_exact: false,
                columns: None,
            }),
        ));
        let dark = Rgb(0x1e, 0x1e, 0x22);
        let mut dl = tris_of(h, Rgb(0x1d, 0x1d, 0x1d));
        make_readable(&mut dl, dark, &d);
        let c = dl.prims[0].color;
        assert!(
            u32::from(c.0) + u32::from(c.1) + u32::from(c.2) > 0x1d * 3,
            "글자가 바탕에 묻힌 채다: {c:?}",
        );
    }

    #[test]
    fn 면은_어두운_바탕에서도_그대로_둔다() {
        // 물량표의 연한 띠나 공간의 비치는 바닥이다. 끌어내면 숯색 덩어리가
        // 되어 그 위의 글자까지 못 읽게 된다.
        let mut d = cadcraft_doc::Drawing::new_metric();
        let h = d.new_handle();
        d.model.push(cadcraft_doc::Entity::new(
            h,
            EntityKind::Hatch(cadcraft_doc::Hatch {
                pattern: "SOLID".into(),
                solid: true,
                loops: vec![],
                scale: 1.0,
                angle: 0.0,
                associative: false,
                style: 0,
                elevation: 0.0,
                gradient: None,
                origin: cadcraft_geom::Vec2::ZERO,
                background: None,
                pattern_lines: vec![],
            }),
        ));
        let dark = Rgb(0x1e, 0x1e, 0x22);
        let band = Rgb(0x1d, 0x1d, 0x1d);
        let mut dl = tris_of(h, band);
        make_readable(&mut dl, dark, &d);
        assert_eq!(dl.prims[0].color, band, "바닥을 밝혀 버렸다");
    }

    #[test]
    fn 모르는_손잡이의_삼각형은_안_건드린다() {
        // 블록 안에서 나온 것은 `model` 에 없다. 모르는 것을 밝혀서 바닥을
        // 망가뜨리는 쪽이 더 나쁘다.
        let d = cadcraft_doc::Drawing::new_metric();
        let band = Rgb(0x1d, 0x1d, 0x1d);
        let mut dl = tris_of(cadcraft_doc::Handle(0xDEAD), band);
        make_readable(&mut dl, Rgb(0x1e, 0x1e, 0x22), &d);
        assert_eq!(dl.prims[0].color, band);
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
        make_readable(&mut dl, WHITE, &cadcraft_doc::Drawing::new_metric());
        assert!(dl.prims[0].aci7, "aci7 을 꺼버렸다 — 배경을 바꾸면 안 뒤집힌다");
    }

    #[test]
    fn 크기가_0_이면_실패를_돌려준다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        assert!(doc.raster("", 0.0, 0.0, 1.0, 0, 100, 0xFFFFFF, false, false, false).is_none());
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
        doc.raster("", 50.0, 25.0, 1.0, 64, 64, 0xFFFFFF, false, false, false).unwrap();
        doc.cache.as_ref().unwrap().5.prims.len()
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
        assert_ne!(doc.cache.as_ref().unwrap().4, doc.revision());
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

    // ───────────────────────── 문서를 엔진이 들 수 있나 ─────────────────────────
    //
    // Bimove 에서 tldraw 를 걷어내면서 "문서를 누가 들고 있나" 를 정했다 —
    // **엔진이 든다.** UI 는 `exec` 로 명령을 보내고 결과를 읽기만 한다.
    //
    // 그 전제가 맞는지 여기서 못 박는다. 그리기·레이어·고르기·읽어오기·
    // 옮기기·되돌리기가 전부 `exec` 하나로 되어야 TS 쪽에 저장소를 따로
    // 만들 필요가 없다. 하나라도 안 되면 그만큼 JS 로 되돌아와야 한다.

    /// `exec` 결과 JSON.
    fn run(doc: &mut Doc, id: &str, params: &str) -> serde_json::Value {
        let out = doc.exec(id, params).unwrap_or_else(|e| panic!("{id} 가 실패했다: {e}"));
        serde_json::from_str(&out).unwrap_or(serde_json::Value::Null)
    }

    /// 방금 그린 것의 handle. 명령마다 담는 칸이 달라서 흔한 자리를 훑는다.
    fn handle_of(v: &serde_json::Value) -> String {
        for key in ["handle", "handles", "added", "created"] {
            match &v[key] {
                serde_json::Value::String(s) => return s.clone(),
                serde_json::Value::Array(a) => {
                    if let Some(serde_json::Value::String(s)) = a.first() {
                        return s.clone();
                    }
                }
                _ => {}
            }
        }
        panic!("handle 을 못 찾았다: {v}")
    }

    #[test]
    fn 벽을_그리고_레이어에_넣고_골라_읽는다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();

        // ① 레이어 — Bimove 의 '도면층' 패널이 하는 일
        run(&mut doc, "layer.new", r#"{"name":"WALL","color":1,"current":true}"#);
        let layers = run(&mut doc, "layer", "{}");
        let names = layers.to_string();
        assert!(names.contains("WALL"), "만든 레이어가 목록에 없다: {names}");

        // ② 벽 하나 — 두께 있는 폴리라인
        let drawn = run(
            &mut doc,
            "pline",
            r#"{"vertices":[[0,0],[5000,0],[5000,3000]],"width":200}"#,
        );
        let wall = handle_of(&drawn);

        // ③ 고르기 — 캔버스를 눌렀을 때
        let picked = doc.pick("", 2500.0, 0.0, 50.0);
        assert_eq!(picked.as_deref(), Some(wall.as_str()), "누른 자리에서 그 벽이 안 잡힌다");

        // ④ 읽어오기 — 속성 패널·물량표가 보는 쪽.
        //
        // `list` 가 아니라 `properties` 를 쓴다. `list` 는 사람이 읽는 글
        // 덩어리를 돌려줘서 (AutoCAD LIST 그대로) 패널을 못 몬다.
        let props = run(&mut doc, "properties", &format!(r#"{{"handles":["{wall}"]}}"#));
        eprintln!("properties → {props}");
        assert!(props.is_object() || props.is_array(), "속성이 구조가 아니다: {props}");
        let text = props.to_string();
        assert!(text.contains("WALL"), "읽어온 것에 레이어가 없다: {text}");

        // ⑤ 옮기기 + 되돌리기 — 둘 다 그냥 명령이다
        let before = doc.revision();
        run(&mut doc, "move", &format!(r#"{{"handles":["{wall}"],"delta":[100,0]}}"#));
        assert!(doc.revision() > before, "옮겼는데 판번호가 안 올랐다");

        // 옮긴 뒤에는 원래 자리에서 안 잡혀야 한다
        assert_ne!(
            doc.pick("", 0.0, 0.0, 10.0).as_deref(),
            Some(wall.as_str()),
            "옮겼는데 옛 자리에 그대로 있다"
        );

        run(&mut doc, "u", "{}");
        assert_eq!(
            doc.pick("", 2500.0, 0.0, 50.0).as_deref(),
            Some(wall.as_str()),
            "되돌렸는데 제자리로 안 왔다"
        );
    }

    #[test]
    fn 고친_도면을_다시_읽을_수_있다() {
        // 저장 → 열기 가 안 되면 그린 게 창을 닫는 순간 사라진다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "layer.new", r#"{"name":"WALL","current":true}"#);
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[5000,0]],"width":200}"#);

        let dxf = doc.to_dxf();
        let again = Doc::open(dxf.as_bytes()).expect("쓴 DXF 를 다시 못 읽었다");

        // 벽이 그 자리에 그대로 있나
        let h = again.pick("", 2500.0, 0.0, 50.0).expect("다시 읽은 도면에 벽이 없다");
        let mut again = again;
        let props = run(&mut again, "properties", &format!(r#"{{"handles":["{h}"]}}"#));
        assert_eq!(props["objects"][0]["layer"], "WALL", "레이어가 안 살아남았다: {props}");
    }

    // ───────────────────── 레이어 패널이 엔진 위에서 서나 ─────────────────────
    //
    // Bimove 의 '도면층' 패널은 지금 tldraw 도형의 meta 를 세어 레이어인 척
    // 한다 — 진짜 레이어가 아니라 셈이다. 엔진에는 진짜가 있다.
    //
    // 패널이 하는 일은 셋이다: **목록을 읽고 · 끄고 켜고 · 끈 게 화면에
    // 반영되고.** 셋째가 제일 중요하다. 명령이 "성공" 을 돌려줘도 그린 게
    // 그대로면 사용자에게는 아무 일도 안 일어난 것이다.

    #[test]
    fn 레이어_목록을_읽는다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "layer.new", r#"{"name":"WALL","color":1}"#);
        run(&mut doc, "layer.new", r#"{"name":"DIM","color":3}"#);

        let v = run(&mut doc, "layer", "{}");
        let names: Vec<String> = v["layers"]
            .as_array()
            .expect(&format!("layers 가 배열이 아니다: {v}"))
            .iter()
            .map(|l| l["name"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(names.contains(&"WALL".to_string()), "만든 레이어가 없다: {names:?}");
        assert!(names.contains(&"DIM".to_string()), "만든 레이어가 없다: {names:?}");
        // 패널이 쓸 칸들 — 이게 없으면 끈 상태를 화면에 되비출 수가 없다.
        let wall = v["layers"].as_array().unwrap().iter().find(|l| l["name"] == "WALL").unwrap();
        assert!(wall.get("on").is_some(), "on 칸이 없다: {wall}");
    }

    #[test]
    fn 레이어를_끄면_화면에서_사라지고_켜면_돌아온다() {
        // 여기가 패널의 전부다. 명령만 돌고 그림이 그대로면 쓸 수가 없다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "layer.new", r#"{"name":"WALL","current":true}"#);
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[200,0],[200,100]],"width":10}"#);

        let on = prim_count(&mut doc);
        run(&mut doc, "layer.set", r#"{"name":"WALL","on":false}"#);
        let off = prim_count(&mut doc);
        assert!(off < on, "레이어를 껐는데 그려지는 게 그대로다 ({on} → {off})");

        run(&mut doc, "layer.set", r#"{"name":"WALL","on":true}"#);
        assert_eq!(prim_count(&mut doc), on, "다시 켰는데 안 돌아왔다");
    }

    #[test]
    fn 레이어_0_도_끄고_켤_수_있다() {
        // 브라우저에서 레이어 "0" 을 껐다 켜니 **안 돌아왔다.** WALL 로 한
        // 앞 시험은 통과했으므로 "0" 이 특별한 것인지 여기서 가린다.
        // ("0" 은 지울 수 없는 기본 레이어다. 모든 도면에 있고, DXF 를 읽으면
        //  대개 제일 많은 게 거기 있다 — 못 켜면 반쯤 못 쓰는 패널이다.)
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let on = prim_count(&mut doc);
        assert!(on > 0, "밑그림이 비었다");

        run(&mut doc, "layer.set", r#"{"name":"0","on":false}"#);
        let off = prim_count(&mut doc);
        assert!(off < on, "레이어 0 을 껐는데 그대로다 ({on} → {off})");

        run(&mut doc, "layer.set", r#"{"name":"0","on":true}"#);
        assert_eq!(prim_count(&mut doc), on, "레이어 0 을 다시 켰는데 안 돌아왔다");
    }

    #[test]
    fn 켤_때_frozen_을_같이_풀어도_돌아온다() {
        // UI 는 켤 때 `{on:true, frozen:false}` 를 보낸다 — 끔만 풀고 동결을
        // 두면 "켰는데 안 나온다" 가 되기 때문이다. 그 조합이 괜찮은지 본다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let on = prim_count(&mut doc);

        run(&mut doc, "layer.set", r#"{"name":"0","on":false}"#);
        assert!(prim_count(&mut doc) < on);

        run(&mut doc, "layer.set", r#"{"name":"0","on":true,"frozen":false}"#);
        assert_eq!(prim_count(&mut doc), on, "frozen 을 같이 보내니 안 돌아온다");
    }

    #[test]
    fn bimove_문서에서도_레이어를_껐다_켤_수_있다() {
        // 브라우저에서 껐다 켜니 **안 돌아왔다.** DXF 로 연 경우는 멀쩡했으므로
        // Bimove 문서로 연 쪽(화면이 실제로 쓰는 길)을 따로 본다 — 거기서는
        // 레이어가 변환하면서 만들어지고, 엔티티 색은 레이어가 아니라
        // 엔티티마다 붙는다. 그 차이가 짚이는 자리다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let all = prim_count(&mut doc);
        assert!(all > 0, "밑그림이 비었다");

        let names: Vec<String> = run(&mut doc, "layer", "{}")["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap_or("").to_string())
            .collect();

        // 레이어마다 껐다 켜서 **전부 제자리로 돌아오는지** 본다. 하나라도
        // 안 돌아오면 패널이 도면을 야금야금 깎아먹는다.
        for name in &names {
            run(&mut doc, "layer.set", &format!(r#"{{"name":"{name}","on":false}}"#));
            let off = prim_count(&mut doc);
            run(&mut doc, "layer.set", &format!(r#"{{"name":"{name}","on":true,"frozen":false}}"#));
            let back = prim_count(&mut doc);
            assert_eq!(back, all, "'{name}' 을 껐다 켜니 안 돌아왔다 ({all} → {off} → {back})");
        }
    }

    // ───────────────────── 커서가 도면에 붙나 ─────────────────────
    //
    // 스냅이 없으면 그린 게 아무 데나 박힌다. 끝점에서 조금 어긋난 벽은
    // 화면에서 안 보이지만 도면에서는 **안 닫힌 벽**이고, 그 위에 면적을
    // 재면 틀린 값이 나온다.
    //
    // 브라우저에서 눈으로 보면 3px 쯤은 가릴 수가 없다 (화면 좌표로 오가며
    // 반올림이 섞인다). 그래서 **엔진 좌표로 정확히** 여기서 못 박는다.

    /// 끝점 스냅 비트 (OSMODE).
    const OS_END: u32 = 1;
    const OS_MID: u32 = 2;

    #[test]
    fn 끝점에_정확히_붙는다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#);

        // 끝점에서 **어긋난** 자리를 묻는다. 붙는 거리 안이면 끝점이 와야 한다.
        let s = doc.snap("", 1007.0, 5.0, 20.0, OS_END, None).expect("끝점을 못 찾았다");
        assert_eq!((s.x, s.y), (1000.0, 0.0), "커서 자리로 돌아왔다 — 안 붙었다");
        assert_eq!(s.name, "Endpoint");
    }

    #[test]
    fn 붙을_거리_밖이면_안_붙는다() {
        // 아무 데나 다 붙으면 스냅이 아니라 방해다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#);
        assert!(doc.snap("", 1500.0, 500.0, 20.0, OS_END, None).is_none());
    }

    #[test]
    fn 중점도_찾는다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#);
        let s = doc.snap("", 505.0, 4.0, 20.0, OS_MID, None).expect("중점을 못 찾았다");
        assert_eq!((s.x, s.y), (500.0, 0.0));
        assert_eq!(s.name, "Midpoint");
    }

    #[test]
    fn 붙을_거리는_도면_단위다() {
        // UI 는 화면 픽셀을 배율로 나눠 넘긴다. 여기서 단위가 어긋나면 확대했을
        // 때 손도 못 대고 축소했을 때 온 도면이 다 붙는다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#);
        // 100 만큼 떨어진 자리: 아퍼처 10 이면 안 붙고, 200 이면 붙는다.
        assert!(doc.snap("", 1100.0, 0.0, 10.0, OS_END, None).is_none());
        assert!(doc.snap("", 1100.0, 0.0, 200.0, OS_END, None).is_some());
    }

    #[test]
    fn 아무것도_안_켜면_안_붙는다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#);
        assert!(doc.snap("", 1000.0, 0.0, 20.0, 0, None).is_none());
    }

    // ───────────────────── 고르고 고치기 ─────────────────────
    //
    // 선택은 **엔진이 든다** (Session.selection). TS 쪽에 따로 두면 둘이
    // 어긋나서, 화면에는 골라져 있는데 지우면 다른 게 지워진다.
    //
    // `pickWindow` 는 **개수만** 돌려준다. 손잡이를 얻으려면 `select` 로
    // 고르고 `properties` 로 읽어야 한다 — 그 왕복이 서는지 본다.

    /// 지금 골라진 것들의 handle.
    fn selected(doc: &mut Doc) -> Vec<String> {
        let v = run(doc, "properties", "{}");
        v["objects"]
            .as_array()
            .map(|a| a.iter().filter_map(|o| o["handle"].as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    #[test]
    fn 점으로_골라_읽는다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#));

        run(&mut doc, "select", r#"{"at":[500,0]}"#);
        assert_eq!(selected(&mut doc), vec![h.clone()], "점으로 고른 게 안 읽힌다");
    }

    #[test]
    fn 네모로_여럿을_고른다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        // **밑그림을 피해 멀리서 한다.** one_line_dxf 의 선이 (0,0)-(100,50) 에
        // 있어서, 원점 근처에 네모를 치면 그것까지 걸린다 — 처음에 그렇게
        // 짰다가 "둘만 골라져야 한다" 가 셋으로 나왔다.
        let a = handle_of(&run(&mut doc, "pline", r#"{"vertices":[[2000,0],[2100,0]]}"#));
        let b = handle_of(&run(&mut doc, "pline", r#"{"vertices":[[2000,200],[2100,200]]}"#));
        run(&mut doc, "pline", r#"{"vertices":[[9000,0],[9100,0]]}"#);   // 멀리 있는 것

        run(&mut doc, "select", r#"{"window":[[1950,-50],[2150,250]]}"#);
        let mut got = selected(&mut doc);
        got.sort();
        let mut want = vec![a, b];
        want.sort();
        assert_eq!(got, want, "네모 안의 둘만 골라져야 한다");
    }

    #[test]
    fn 걸치기와_포함은_다르다() {
        // 오토캐드에서 왼→오 드래그는 **완전히 들어온 것**, 오→왼은 **걸친 것**.
        // 둘이 같으면 드래그 방향이 아무 뜻도 없어진다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        // 밑그림에서 멀리 (위 주석과 같은 이유).
        run(&mut doc, "pline", r#"{"vertices":[[2000,0],[3000,0]]}"#);

        // 선의 **일부만** 덮는 네모
        run(&mut doc, "select", r#"{"window":[[1950,-50],[2500,50]]}"#);
        let contained = selected(&mut doc).len();
        run(&mut doc, "select", r#"{"clear":true}"#);
        run(&mut doc, "select", r#"{"window":[[1950,-50],[2500,50]],"crossing":true}"#);
        let crossing = selected(&mut doc).len();

        assert_eq!(contained, 0, "일부만 덮었는데 포함으로 잡혔다");
        assert_eq!(crossing, 1, "걸쳤는데 안 잡혔다");
    }

    #[test]
    fn 고른_것을_지우고_되돌린다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#));
        let before = prim_count(&mut doc);

        run(&mut doc, "select", r#"{"at":[500,0]}"#);
        // handles 를 안 주면 **골라진 것**을 지운다 — 그게 UI 가 하는 짓이다.
        run(&mut doc, "erase", "{}");
        assert!(prim_count(&mut doc) < before, "지웠는데 그대로다");
        assert_eq!(doc.pick("", 500.0, 0.0, 20.0), None);

        run(&mut doc, "u", "{}");
        assert_eq!(doc.pick("", 500.0, 0.0, 20.0).as_deref(), Some(h.as_str()), "되돌렸는데 안 살아났다");
    }

    #[test]
    fn 고른_것을_옮긴다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#));
        run(&mut doc, "select", r#"{"at":[500,0]}"#);
        run(&mut doc, "move", r#"{"delta":[0,300]}"#);

        assert_eq!(doc.pick("", 500.0, 0.0, 20.0), None, "옛 자리에 그대로 있다");
        assert_eq!(doc.pick("", 500.0, 300.0, 20.0).as_deref(), Some(h.as_str()), "새 자리에 없다");
    }

    #[test]
    fn 고른_것을_복사한다() {
        // 끌면서 Ctrl 을 누르면 옮기는 게 아니라 복사다. 원본이 **그대로 있는
        // 것**까지 봐야 한다 — 안 그러면 "복사" 가 사실은 옮기기다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "pline", r#"{"vertices":[[2000,0],[3000,0]]}"#));
        run(&mut doc, "select", r#"{"at":[2500,0]}"#);
        run(&mut doc, "copy", r#"{"delta":[0,500]}"#);

        assert_eq!(doc.pick("", 2500.0, 0.0, 20.0).as_deref(), Some(h.as_str()), "원본이 사라졌다");
        assert!(doc.pick("", 2500.0, 500.0, 20.0).is_some(), "사본이 없다");
    }

    #[test]
    fn 복사를_되돌리면_사본만_없어진다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "pline", r#"{"vertices":[[2000,0],[3000,0]]}"#));
        run(&mut doc, "select", r#"{"at":[2500,0]}"#);
        run(&mut doc, "copy", r#"{"delta":[0,500]}"#);
        run(&mut doc, "u", "{}");

        assert_eq!(doc.pick("", 2500.0, 500.0, 20.0), None, "사본이 안 없어졌다");
        assert_eq!(doc.pick("", 2500.0, 0.0, 20.0).as_deref(), Some(h.as_str()), "원본까지 없어졌다");
    }

    #[test]
    fn 고른_것의_범위를_읽는다() {
        // 화면에 "이게 골라졌다" 를 보여주려면 네모를 알아야 한다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,500]]}"#);
        run(&mut doc, "select", r#"{"at":[500,250]}"#);

        let v = run(&mut doc, "properties", "{}");
        let b = &v["objects"][0]["bounds"];
        assert!(b.is_array(), "범위가 없다: {v}");
        assert_eq!(b[0][0].as_f64(), Some(0.0));
        assert_eq!(b[1][0].as_f64(), Some(1000.0));
    }

    #[test]
    fn 비우면_아무것도_안_골라진다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "pline", r#"{"vertices":[[0,0],[1000,0]]}"#);
        run(&mut doc, "select", r#"{"at":[500,0]}"#);
        assert_eq!(selected(&mut doc).len(), 1);
        run(&mut doc, "select", r#"{"clear":true}"#);
        assert_eq!(selected(&mut doc).len(), 0, "비웠는데 남아 있다");
    }

    // ───────────────── 문·창 · 치수 · 공간을 엔진 명령으로 ─────────────────
    //
    // Bimove 가 실제로 쓰는 도형은 벽·선만이 아니다. 저장을 엔진으로 넘기려면
    // 이 셋이 다 그려져야 한다. 셋의 길이 서로 다르다:
    //
    //   치수   엔진에 **네이티브로 있다** (DIMENSION) — 명령 하나
    //   공간   닫힌 폴리라인 + 해치 + 글자 — 명령 셋을 엮는다
    //   문·창  엔진에 그런 개념이 없다. 벽을 가리는 사각형 + 잼브 + 스윙 호를
    //          손으로 엮는다 (opening.rs 가 읽을 때 하는 것과 같은 모양)
    //
    // 여기서 보는 건 **엮을 수 있나** 다. 명령이 있어도 돌려주는 모양이
    // 다르면 UI 가 그 위에 설 수가 없다.

    #[test]
    fn 치수를_긋는다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let before = prim_count(&mut doc);
        let v = run(&mut doc, "dimaligned", r#"{"p1":[2000,0],"p2":[3000,0],"at":[2500,400]}"#);
        assert!(prim_count(&mut doc) > before, "치수를 그었는데 화면에 안 나온다");

        let h = handle_of(&v);
        let p = run(&mut doc, "properties", &format!(r#"{{"handles":["{h}"]}}"#));
        let kind = p["objects"][0]["type"].as_str().unwrap_or("");
        assert!(kind.contains("Dimension"), "치수가 아닌 걸로 들어갔다: {kind}");
    }

    #[test]
    fn 치수는_잰_두_점을_들고_있다() {
        // 숫자를 우리가 적어 넣지 않는다. 치수는 **두 점**을 들고 있고, 길이는
        // 그릴 때 거기서 잰다 (crates/render/src/dim.rs 의 `g.measurement = meas`).
        // 그래서 저장된 `measurement` 는 0 이다 — 그걸 보고 "길이를 모른다" 고
        // 읽으면 안 된다. 점이 맞으면 옮겨도 숫자가 따라간다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "dimaligned", r#"{"p1":[2000,0],"p2":[3000,0],"at":[2500,400]}"#));
        let g = &run(&mut doc, "properties", &format!(r#"{{"handles":["{h}"]}}"#))["objects"][0]["geometry"];
        assert_eq!(g["kind"], "aligned");
        assert_eq!((g["p13"]["x"].as_f64(), g["p13"]["y"].as_f64()), (Some(2000.0), Some(0.0)));
        assert_eq!((g["p14"]["x"].as_f64(), g["p14"]["y"].as_f64()), (Some(3000.0), Some(0.0)));
    }

    #[test]
    fn 공간은_닫힌_폴리라인에_해치를_얹어_만든다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let before = prim_count(&mut doc);

        let poly = handle_of(&run(
            &mut doc,
            "pline",
            r#"{"vertices":[[2000,0],[3000,0],[3000,800],[2000,800]],"closed":true}"#,
        ));
        // 해치는 **그 도형을 가리켜** 건다. 내부 점으로 걸면 도면이 빽빽할 때
        // 엉뚱한 칸을 채운다.
        run(&mut doc, "hatch", &format!(r#"{{"handles":["{poly}"],"pattern":"SOLID"}}"#));
        run(&mut doc, "text", r#"{"at":[2500,400],"text":"거실","height":120,"justify":"MC"}"#);

        assert!(prim_count(&mut doc) > before, "공간을 만들었는데 화면에 안 나온다");

        // 셋이 다 들어갔나
        let all = run(&mut doc, "select", r#"{"window":[[1900,-100],[3100,900]]}"#);
        let _ = all;
        let kinds: Vec<String> = run(&mut doc, "properties", "{}")["objects"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|o| o["type"].as_str().map(str::to_string))
            .collect();
        assert!(kinds.iter().any(|k| k.contains("Polyline")), "폴리라인이 없다: {kinds:?}");
        assert!(kinds.iter().any(|k| k.contains("Hatch")), "해치가 없다: {kinds:?}");
        assert!(kinds.iter().any(|k| k.contains("Text")), "글자가 없다: {kinds:?}");
    }

    #[test]
    fn 문은_가림막과_잼브와_스윙호로_엮인다() {
        // 엔진에 '문' 이 없으므로 손으로 엮는다. opening.rs 가 읽을 때 만드는
        // 것과 같은 모양이라, 그려 넣은 문이 다시 읽혀도 같게 보인다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let before = prim_count(&mut doc);

        // 벽을 가리는 사각형 (문이 뚫린 자리)
        run(&mut doc, "rectang", r#"{"p1":[2400,-50],"p2":[2800,50]}"#);
        // 잼브 둘
        run(&mut doc, "line", r#"{"points":[[2400,-50],[2400,50]]}"#);
        run(&mut doc, "line", r#"{"points":[[2800,-50],[2800,50]]}"#);
        // 문짝 + 스윙 호
        run(&mut doc, "line", r#"{"points":[[2400,0],[2400,400]]}"#);
        let arc = handle_of(&run(
            &mut doc,
            "arc",
            r#"{"center":[2400,0],"radius":400,"start":0,"end":90}"#,
        ));

        assert!(prim_count(&mut doc) > before, "문을 그렸는데 화면에 안 나온다");
        let p = run(&mut doc, "properties", &format!(r#"{{"handles":["{arc}"]}}"#));
        assert_eq!(p["objects"][0]["type"], "Arc", "스윙이 호가 아니다: {p}");
    }

    #[test]
    fn 여러_명령을_한_번에_되돌릴_수_있다() {
        // 공간 하나가 명령 셋(폴리라인·해치·글자), 문 하나가 다섯이다. 묶지
        // 않으면 **되돌리기를 다섯 번** 눌러야 문 하나가 사라진다 — 쓸 수가 없다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let before = prim_count(&mut doc);

        run(&mut doc, "undo", r#"{"mode":"begin"}"#);
        run(&mut doc, "pline", r#"{"vertices":[[2000,0],[3000,0],[3000,800],[2000,800]],"closed":true}"#);
        run(&mut doc, "text", r#"{"at":[2500,400],"text":"거실","height":120,"justify":"MC"}"#);
        run(&mut doc, "line", r#"{"points":[[2000,0],[3000,800]]}"#);
        run(&mut doc, "undo", r#"{"mode":"end"}"#);

        let after = prim_count(&mut doc);
        assert!(after > before, "그렸는데 화면에 안 나온다");

        // **한 번**이면 셋이 다 사라져야 한다.
        run(&mut doc, "u", "{}");
        assert_eq!(prim_count(&mut doc), before, "한 번에 안 돌아갔다 ({before} → {after} → {})", prim_count(&mut doc));
    }

    #[test]
    fn 묶음을_되돌린_뒤_다시_실행하면_통째로_돌아온다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let before = prim_count(&mut doc);
        run(&mut doc, "undo", r#"{"mode":"begin"}"#);
        run(&mut doc, "pline", r#"{"vertices":[[2000,0],[3000,0]]}"#);
        run(&mut doc, "pline", r#"{"vertices":[[2000,400],[3000,400]]}"#);
        run(&mut doc, "undo", r#"{"mode":"end"}"#);
        let after = prim_count(&mut doc);

        run(&mut doc, "u", "{}");
        assert_eq!(prim_count(&mut doc), before);
        run(&mut doc, "redo", "{}");
        assert_eq!(prim_count(&mut doc), after, "다시 실행이 반만 돌아왔다");
    }

    // ──────────────── 저장본이 DXF 를 건너도 같은가 ────────────────
    //
    // 저장을 엔진으로 넘기기 전에 반드시 재야 한다. 한 번 넘기면 한의 실제
    // 도면이 그 포맷으로 가고, 그때 "벽이 까맣게 뭉개졌다" 를 알게 되면
    // 되돌릴 방법이 없다.
    //
    // 브라우저로 11 개를 돌려 봤더니 벽만 있는 도면이 다시 읽을 때 **검게
    // 칠해진 덩어리**가 됐다. 여기서 그 자리를 좁힌다.

    /// 지금 모형공간의 엔티티 종류별 개수.
    fn kinds(doc: &mut Doc) -> std::collections::BTreeMap<String, usize> {
        run(doc, "select", r#"{"window":[[-1e9,-1e9],[1e9,1e9]],"crossing":true}"#);
        let mut m = std::collections::BTreeMap::new();
        if let Some(a) = run(doc, "properties", "{}")["objects"].as_array() {
            for o in a {
                *m.entry(o["type"].as_str().unwrap_or("?").to_string()).or_insert(0) += 1;
            }
        }
        m
    }

    #[test]
    fn bimove_벽이_dxf_를_건너도_mline_으로_남는다() {
        // 벽은 MLINE 으로 들어간다 (wall.rs). DXF 를 거쳐 돌아올 때 그게
        // 다른 것으로 바뀌면 **두께가 면으로 칠해져** 도면이 까맣게 뭉개진다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let before = kinds(&mut doc);
        assert!(before.contains_key("MLine"), "벽이 MLINE 이 아니다: {before:?}");

        let dxf = doc.to_dxf();
        let mut again = Doc::open(dxf.as_bytes()).expect("쓴 DXF 를 다시 못 읽었다");
        let after = kinds(&mut again);

        assert!(
            after.contains_key("MLine"),
            "DXF 를 건너니 MLINE 이 사라졌다\n  전 {before:?}\n  후 {after:?}",
        );
    }

    #[test]
    fn bimove_저장본이_dxf_를_건너도_그리는_양이_비슷하다() {
        // 종류가 같아도 그려지는 양이 확 달라지면 뭔가 잘못 해석된 것이다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let before = prim_count(&mut doc);

        let dxf = doc.to_dxf();
        let mut again = Doc::open(dxf.as_bytes()).unwrap();
        let after = prim_count(&mut again);

        let ratio = after as f64 / before.max(1) as f64;
        assert!(
            (0.5..2.0).contains(&ratio),
            "DXF 를 건너며 그릴 거리가 {ratio:.1} 배로 달라졌다 ({before} → {after})",
        );
    }

    /// 한의 실제 도면에서 뽑은 DXF — 벽만 있는 것 (MLINE 4 개).
    ///
    /// 브라우저에서 이걸 다시 읽으니 벽이 **까맣게 칠해진 덩어리**가 됐다.
    /// 픽스처(벽 하나)로는 재현이 안 돼서 실제 바이트를 들여온다.
    const WALLS_DXF: &str = include_str!("../tests/walls.dxf");

    /// 같은 도면을 **고친 뒤에 다시 받은 것.** 손으로 만든 게 아니라 브라우저가
    /// 실제로 내려준 바이트다. 두 파일의 차이는 벽 스타일의 채움 색 한 군데다 —
    /// ACI 7 → 255 ([`cadcraft_color::nearest_aci`]).
    const WALLS_FIXED_DXF: &str = include_str!("../tests/walls-fixed.dxf");

    #[test]
    fn 옛_내보내기는_채움이_aci_7_이라_검게_나온다() {
        // **증거를 남겨 둔다.** 이 바이트는 고치기 전 쓰기가 낸 것이고, 그걸
        // 읽으면 까맣다 — AutoCAD 도 흰 바탕에서 똑같이 검게 그린다. 즉 파일이
        // 틀린 것이고, 읽기 쪽을 고칠 자리가 아니다. 나중에 누가 "ACI 7 을
        // 흰색으로 읽자" 로 고치려 들면 여기서 왜 아닌지 읽게 된다.
        let at = WALLS_DXF.rfind("BIMOVE_WALL_200000").unwrap();
        assert!(
            WALLS_DXF[at..].contains("\r\n 62\r\n7\r\n"),
            "들여온 바이트에 채움 62=7 이 없다 — 픽스처가 바뀌었다",
        );

        let mut doc = Doc::open(WALLS_DXF.as_bytes()).expect("DXF 를 못 읽었다");
        assert!(ink_fraction(&mut doc) > 0.30, "옛 내보내기가 검지 않다 — 다른 데가 달라졌다");
    }

    #[test]
    fn 실제_벽_도면을_다시_읽어도_뭉개지지_않는다() {
        let mut doc = Doc::open(WALLS_FIXED_DXF.as_bytes()).expect("DXF 를 못 읽었다");
        let k = kinds(&mut doc);
        assert_eq!(k.get("MLine").copied().unwrap_or(0), 4, "MLINE 4 개가 아니다: {k:?}");

        // **면이 있는 건 정상이다.** 벽은 흰색으로 채워 속을 가린다
        // (`bimove.rs` 의 기본 채움). 처음에 삼각형 수로 쟀다가 틀렸다 —
        // 고친 뒤에도 삼각형은 그대로 204 개이고, 달라진 건 **색**이다.
        // 그래서 채우는 면마다 색을 본다.
        let f = doc.build("", 1.0);
        let mut filled = 0;
        for p in f.prims.chunks_exact(PRIM_STRIDE) {
            if p[0] != KIND_TRIS {
                continue;
            }
            filled += 1;
            let (c, aci7) = (p[1] as u32, p[5] != 0);
            let (r, g, b) = (c >> 16 & 0xFF, c >> 8 & 0xFF, c & 0xFF);
            // aci7 은 색이 아니라 "바탕과 반대" 다 — 흰 종이에서 검정이 된다.
            assert!(!aci7, "벽 채움이 ACI 7 이다 — 흰 종이에서 검게 뒤집힌다");
            assert!(
                r > 200 && g > 200 && b > 200,
                "벽 채움이 어둡다 (#{r:02X}{g:02X}{b:02X}) — 도면이 덩어리로 뭉개진다",
            );
        }
        assert!(filled > 0, "벽에 채움이 아예 없다 — 속이 비어 뒤가 보인다");
    }

    #[test]
    fn 실제_벽_도면이_화면을_덮지_않는다() {
        // 사람이 보는 건 결국 픽셀이다. 벽 4 줄짜리 도면이 화면을 까맣게
        // 덮으면 **그게 고장이다** — 브라우저에서 실제로 그랬다.
        let mut doc = Doc::open(WALLS_FIXED_DXF.as_bytes()).unwrap();
        let f = doc.build("", 1.0);
        let [minx, miny, maxx, maxy] = f.bounds;
        let (w, h) = (600u32, 400u32);
        let scale = ((f64::from(w) - 48.0) / (maxx - minx).max(1e-9))
            .min((f64::from(h) - 48.0) / (maxy - miny).max(1e-9));

        let r = doc
            .raster("", (minx + maxx) / 2.0, (miny + maxy) / 2.0, scale, w, h, 0xFFFFFF, false, false, false)
            .expect("못 그렸다");
        let px = &r.rgba;
        let mut ink = 0usize;
        for p in px.chunks_exact(4) {
            if p[0] < 240 || p[1] < 240 || p[2] < 240 {
                ink += 1;
            }
        }
        let frac = ink as f64 / f64::from(w * h);
        assert!(frac < 0.30, "벽 4 줄인데 화면의 {:.0}% 가 칠해졌다", frac * 100.0);
    }

    /// 지금 도면을 한 장 그려 **칠해진 픽셀 비율**을 돌려준다 (0..1).
    fn ink_fraction(doc: &mut Doc) -> f64 {
        let f = doc.build("", 1.0);
        let [minx, miny, maxx, maxy] = f.bounds;
        let (w, h) = (600u32, 400u32);
        let scale = ((f64::from(w) - 48.0) / (maxx - minx).max(1e-9))
            .min((f64::from(h) - 48.0) / (maxy - miny).max(1e-9));
        let Some(r) = doc.raster("", (minx + maxx) / 2.0, (miny + maxy) / 2.0, scale, w, h, 0xFFFFFF, false, false, false)
        else {
            return 0.0;
        };
        let ink = r.rgba.chunks_exact(4).filter(|p| p[0] < 240 || p[1] < 240 || p[2] < 240).count();
        ink as f64 / f64::from(w * h)
    }

    /// 벽만 있는 Bimove 문서 — 다른 도형에 묻히지 않게.
    fn walls_only_json() -> String {
        r##"{"version":1,
          "pages":[{"id":"page:main","name":"모형","index":"a1"}],
          "shapes":[
            {"id":"shape:w1","type":"wall","x":0,"y":0,"rotation":0,"index":"a1",
             "parentId":"page:main","props":{"x2":4000,"y2":0,"thickness":200}},
            {"id":"shape:w2","type":"wall","x":4000,"y":0,"rotation":0,"index":"a2",
             "parentId":"page:main","props":{"x2":0,"y2":3000,"thickness":200}}],
          "records":{},"skipped":0,
          "source":{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":2}}"##
            .to_string()
    }

#[test]
    fn 그어_넣는_지시선도_mm_크기로_나온다() {
        // 스타일을 테이블에 넣어 두는 것만으로는 모자란다. `mleader` 명령은
        // 머리말의 `CMLEADERSTYLE` 을 보고 스타일을 고르므로 (annotate.rs 의
        // `mleader_kind`), 머리말을 안 돌리면 **그어 넣은 것만** 화살촉 0.18 로
        // 나와서 안 보인다 — 저장본에서 온 것은 멀쩡한 채로.
        let mut doc = Doc::from_bimove(&walls_only_json(), "").unwrap();
        doc.exec("mleader", r#"{"points":[[0,0],[500,500]],"text":"여기"}"#).unwrap();
        let m = doc
            .drawing()
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::MLeader(m) => Some(m.clone()),
                _ => None,
            })
            .expect("지시선이 안 들어갔다");
        assert_eq!(m.arrow_size, 12.0, "화살촉이 inch 크기다");
        assert_eq!(m.text.as_ref().expect("라벨이 없다").height, 20.0);
        assert!(m.dogleg > 1.0, "랜딩 꺾임이 inch 크기다: {}", m.dogleg);
    }

    #[test]
    fn 그어_넣은_지시선이_정체성을_달고_저장본으로_돌아온다() {
        // 명령으로 그은 것에는 정체성이 **안 붙는다** — 붙이는 것은 화면
        // 쪽이다 (`cadIdentity.ts`). 여기서는 붙였을 때 역변환이 읽는지를 본다.
        let mut doc = Doc::from_bimove(&walls_only_json(), "").unwrap();
        doc.exec("mleader", r#"{"points":[[0,0],[500,500]],"text":"여기"}"#).unwrap();
        let h = doc
            .drawing()
            .model
            .iter()
            .find(|e| matches!(e.kind, EntityKind::MLeader(_)))
            .map(|e| e.handle.hex())
            .unwrap();
        doc.exec(
            "xdata.set",
            &format!(
                r#"{{"handles":["{h}"],"data":{{"kind":"leader","ids":["shape:bimove:leader:x"]}}}}"#
            ),
        )
        .unwrap();
        let back = doc.to_bimove(&walls_only_json()).expect("되돌리기 실패");
        assert!(
            back.json.contains("shape:bimove:leader:x"),
            "그은 지시선이 저장본에 없다: {:?}",
            back.notes,
        );
    }

    #[test]
    fn 벽_속은_흰색이다() {
        // 평면도에서 벽은 **가는 윤곽선 두 줄 + 흰 속**이다 (tldraw 쪽
        // `WallShape.tsx` 가 그렇게 그린다: fill `#fff`, stroke `#222`).
        // 엔진 화면에서는 **까만 덩어리**로 나왔다 — 브라우저에서 벽 가운데
        // 픽셀을 읽으니 (0,0,0) 이었다.
        //
        // `bimove.rs` 는 fill 기본값으로 흰색을 넣는다. 그러니 흰 것이
        // 어디서 검게 바뀌는지를 여기서 가른다: 엔진이 그릴 때부터 검으면
        // 엔진 쪽, 여기서 희면 화면 쪽 문제다.
        let mut doc = Doc::from_bimove(&walls_only_json(), "").unwrap();
        // 첫 벽은 (0,0)-(4000,0), 두께 200. 그 한가운데를 본다.
        let r = doc
            .raster("", 2000.0, 0.0, 0.08, 400, 300, 0xFFFFFF, false, false, false)
            .expect("래스터 실패");
        let (rr, gg, bb) = mid_rgb(&r);
        assert!(
            rr > 200 && gg > 200 && bb > 200,
            "벽 속이 흰색이 아니다: ({rr},{gg},{bb})",
        );
    }

    #[test]
    fn 벽만_있는_도면이_dxf_를_건너며_검게_칠해지지_않는다() {
        // 브라우저에서 본 것이 이것이다: 벽만 있는 저장본을 열면 가는 윤곽선인데,
        // DXF 로 꺼내 다시 읽으면 **까맣게 칠해진 덩어리**가 됐다 (잉크 41 배).
        //
        // 자리는 `bimove.rs` 의 벽 채우기다 — 메모리에서는 **참 흰색**
        // (`Color::True(255,255,255)`) 이라 흰 바탕에서 안 보인다. 그런데 DXF 에는
        // `62 = 7` (ACI 7) 로 나간다. ACI 7 은 **밝은 바탕에서 검정**이다
        // (CAD 관례). 그래서 돌아오면 벽 속이 까맣다.
        let mut doc = Doc::from_bimove(&walls_only_json(), "").unwrap();
        let direct = ink_fraction(&mut doc);

        let dxf = doc.to_dxf();
        let mut again = Doc::open(dxf.as_bytes()).unwrap();
        let round = ink_fraction(&mut again);

        assert!(
            round < direct * 2.0 + 0.02,
            "DXF 를 건너니 칠해진 양이 {:.1}% → {:.1}% 로 늘었다",
            direct * 100.0, round * 100.0,
        );
    }

    #[test]
    fn 벽이_dxf_를_건너며_검게_칠해지지_않는다() {
        // 브라우저에서 본 것: 저장본을 열면 벽이 가는 윤곽선인데, 그걸 DXF 로
        // 꺼내 다시 읽으면 **까맣게 칠해진 덩어리**가 된다. 같은 도면을 두 길로
        // 그려 나란히 본다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let direct = ink_fraction(&mut doc);

        let dxf = doc.to_dxf();
        let mut again = Doc::open(dxf.as_bytes()).unwrap();
        let round = ink_fraction(&mut again);

        assert!(
            round < direct * 2.0 + 0.05,
            "DXF 를 건너니 칠해진 양이 {:.0}% → {:.0}% 로 늘었다 — 벽 두께가 면으로 칠해진 것이다",
            direct * 100.0, round * 100.0,
        );
    }

    /// 도면에 있는 모든 엔티티의 `BIMOVE` 정체성. `kind` 별로 id 를 모은다.
    ///
    /// 손잡이를 짚어 가며 읽지 않는 이유는, 문 하나가 엔티티 다섯이고 그 중
    /// 어느 것이 몇 번째인지는 이 테스트가 알 바가 아니기 때문이다. **전부**
    /// 훑어서 결과만 본다.
    fn identities(doc: &mut Doc) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
        run(doc, "selectall", "{}");
        let v = run(doc, "xdata", "{}");
        let mut out: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> = Default::default();
        for o in v["objects"].as_array().expect("objects 가 없다") {
            let d = &o["data"];
            let Some(kind) = d["kind"].as_str() else { continue };
            let Some(ids) = d["ids"].as_array() else { continue };
            let e = out.entry(kind.to_string()).or_default();
            for id in ids.iter().filter_map(|x| x.as_str()) {
                e.insert(id.to_string());
            }
        }
        out
    }

    #[test]
    fn bimove_도형의_정체성이_엔티티에_붙는다() {
        // 여기가 "객체 정체성" 의 핵심이다. 문 하나는 엔티티 다섯, 공간
        // 하나는 면·테두리·글자로 흩어지고 벽은 거꾸로 여럿이 하나로 묶인다.
        // 그 조각들이 어느 Bimove 도형의 것인지가 **도면 안에** 있어야 한다 —
        // 없으면 문짝 선 하나만 골라 지울 수 있고, 물량표가 "문 1개" 를
        // 되짚을 길이 없다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let got = identities(&mut doc);

        for (kind, id) in [("wall", "shape:w"), ("door", "shape:d"), ("dim", "shape:m"), ("zone", "shape:z"), ("boq", "shape:t")] {
            let ids = got.get(kind).unwrap_or_else(|| panic!("{kind} 의 정체성이 아예 없다: {got:?}"));
            assert!(ids.contains(id), "{kind} 에 {id} 가 없다: {ids:?}");
        }
    }

    #[test]
    fn 정체성이_dxf_를_건너도_살아남는다() {
        // 메모리에서만 들고 있으면 저장하는 순간 문이 선 다섯으로 흩어진다.
        // xdata 를 고른 이유가 이것이다 — 객체에 붙어 파일까지 나간다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let before = identities(&mut doc);
        assert!(!before.is_empty(), "들어가기도 전에 비었다");

        let dxf = doc.to_dxf();
        let mut again = Doc::open(dxf.as_bytes()).expect("쓴 DXF 를 다시 못 읽었다");
        let after = identities(&mut again);

        assert_eq!(after, before, "DXF 를 건너며 정체성이 달라졌다");
    }

    #[test]
    fn 문의_조각_다섯이_같은_덩어리다() {
        // 조각 하나를 집으면 나머지 넷을 찾을 수 있어야 한다. 그 길이 이것이다:
        // 정체성이 같은 엔티티를 모은다.
        let mut doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        run(&mut doc, "selectall", "{}");
        let v = run(&mut doc, "xdata", "{}");

        let door: Vec<&serde_json::Value> = v["objects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["data"]["kind"] == "door")
            .collect();
        assert_eq!(door.len(), 5, "문의 조각이 {}개다 — 가림 + 잼브 둘 + 문짝 + 호 다섯이어야 한다", door.len());
        for o in &door {
            assert_eq!(o["data"]["ids"], serde_json::json!(["shape:d"]));
        }
    }

    /// `bimove_json()` 의 문에서 **문만 있는** 자리 (엔진 좌표).
    ///
    /// 문은 캔버스 (50,60) 에 폭 40 · 두께 20 으로 서 있다. y 가 뒤집히므로
    /// 엔진에서 잼브는 x = 30 과 70, y = -70..-50 이다. 그 중 y = -65 를
    /// 고른 이유는 **공간 테두리를 피하려는 것**이다 — 공간이 y = -60 을 따라
    /// 지나가고 문보다 **나중에** 그려져서, y = -60 을 찍으면 공간이 잡힌다.
    /// (그걸로 처음에 테스트가 떨어졌다. `group` 은 맞게 돌고 있었다.)
    const DOOR_PIECE: (f64, f64) = (30.0, -65.0);

    #[test]
    fn 문의_한_조각을_찍으면_다섯이_다_잡힌다() {
        // 화면이 하는 일 그대로다: `pick` 으로 하나 집고 `group` 으로 펼친다.
        // 이게 "문짝 선 하나만 지워진다" 를 끝내는 길이다.
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();

        let h = doc.pick("", DOOR_PIECE.0, DOOR_PIECE.1, 1.0).expect("문 조각을 못 집었다");
        let g = doc.group(&h);
        assert_eq!(g.len(), 5, "묶음이 {}개다 — 가림 + 잼브 둘 + 문짝 + 호 다섯이어야 한다: {g:?}", g.len());
        assert!(g.contains(&h), "찍은 것이 묶음에 없다");
    }

    #[test]
    fn 정체성_없는_선은_묶음이_없다() {
        // DXF 에서 그냥 열은 도면이다. 빈 배열이어야 부르는 쪽이 "묶음 없음"
        // 과 "혼자인 객체" 를 구분할 수 있다.
        let doc = Doc::open(&one_line_dxf()).unwrap();
        // 선은 (0,0)→(100,50) 이다. 가운데를 집는다.
        let h = doc.pick("", 50.0, 25.0, 10.0).expect("선을 못 집었다");
        assert!(doc.group(&h).is_empty(), "정체성 없는 선에 묶음이 생겼다");
    }

    #[test]
    fn 없는_손잡이에는_빈_묶음() {
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        assert!(doc.group("ffffff").is_empty());
        assert!(doc.group("zzz").is_empty(), "16진수가 아닌 것에 터지지 않아야 한다");
    }

    #[test]
    fn 묶음이_dxf_를_건너도_그대로다() {
        let doc = Doc::from_bimove(&bimove_json(), "").unwrap();
        let before = doc.pick("", DOOR_PIECE.0, DOOR_PIECE.1, 1.0).map(|h| doc.group(&h).len());

        let dxf = doc.to_dxf();
        let again = Doc::open(dxf.as_bytes()).expect("쓴 DXF 를 다시 못 읽었다");
        let h = again
            .pick("", DOOR_PIECE.0, DOOR_PIECE.1, 1.0)
            .expect("다시 읽은 도면에 문 조각이 없다");
        assert_eq!(Some(again.group(&h).len()), before, "DXF 를 건너며 묶음 크기가 달라졌다");
    }

    #[test]
    fn 벽_채움은_aci_7_로_안_나간다() {
        // 위 두 테스트는 **그려진 양**을 보니 렌더가 달라지면 같이 흔들린다.
        // 고친 자리 자체를 못 박아 둔다: 벽 스타일의 채움 색(62) 은 7 이 아니다.
        // 7 은 색이 아니라 "바탕과 반대" 라서, 흰 종이에서 검정이 된다.
        let mut doc = Doc::from_bimove(&walls_only_json(), "").unwrap();
        let dxf = doc.to_dxf();

        let at = dxf.rfind("BIMOVE_WALL").expect("벽 스타일이 안 나갔다");
        let style = &dxf[at..];
        // 스타일 기록 안의 **첫** 62 가 채움 색이다 (그 뒤는 선마다의 색).
        let fill = style
            .lines()
            .skip_while(|l| l.trim() != "62")
            .nth(1)
            .expect("채움 색이 없다")
            .trim()
            .to_string();
        assert_ne!(fill, "7", "벽 채움이 ACI 7 로 나갔다 — 흰 종이에서 검게 뒤집힌다");
    }

    #[test]
    fn 레이어별로_몇_개인지_셀_수_있다() {
        // 패널이 레이어마다 옆에 붙이는 숫자. properties 로 센다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        run(&mut doc, "layer.new", r#"{"name":"WALL","current":true}"#);
        let a = handle_of(&run(&mut doc, "circle", r#"{"center":[0,0],"radius":50}"#));
        let b = handle_of(&run(&mut doc, "circle", r#"{"center":[300,0],"radius":50}"#));

        let v = run(&mut doc, "properties", &format!(r#"{{"handles":["{a}","{b}"]}}"#));
        let on_wall = v["objects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["layer"] == "WALL")
            .count();
        assert_eq!(on_wall, 2, "레이어별 개수를 못 센다: {v}");
    }

    #[test]
    fn 마감재_같은_의미_정보가_저장을_건너도_살아남는다() {
        // Bimove 도형은 CAD 프리미티브가 아니다 — 벽에는 마감재가, 공간에는
        // 면적과 마감 데이터가 붙는다. 그게 엔티티에 안 붙어 있으면 DXF 로
        // 내보내는 순간 떨어져 나가고, 돌아온 도면은 선만 남는다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "pline", r#"{"vertices":[[0,0],[5000,0]],"width":200}"#));

        let meta = r#"{"kind":"wall","재질":"석고보드 9.5T","두께":200,"단가":12500}"#;
        run(&mut doc, "xdata.set", &format!(r#"{{"handles":["{h}"],"data":{meta}}}"#));

        // ① 그 자리에서 바로 읽힌다
        let got = run(&mut doc, "xdata", &format!(r#"{{"handles":["{h}"]}}"#));
        assert_eq!(got["objects"][0]["data"]["재질"], "석고보드 9.5T", "방금 넣은 걸 못 읽는다: {got}");

        // ② 되돌리기에 들어간다
        run(&mut doc, "u", "{}");
        let after_undo = run(&mut doc, "xdata", &format!(r#"{{"handles":["{h}"]}}"#));
        assert!(after_undo["objects"][0]["data"].is_null(), "되돌렸는데 남아 있다: {after_undo}");
        run(&mut doc, "redo", "{}");

        // ③ **저장하고 다시 읽어도 살아 있다** — 여기가 진짜다
        let dxf = doc.to_dxf();
        let mut again = Doc::open(dxf.as_bytes()).expect("쓴 DXF 를 다시 못 읽었다");
        let h2 = again.pick("", 2500.0, 0.0, 50.0).expect("다시 읽은 도면에 벽이 없다");
        let back = run(&mut again, "xdata", &format!(r#"{{"handles":["{h2}"]}}"#));
        assert_eq!(back["objects"][0]["data"]["재질"], "석고보드 9.5T", "저장을 건너며 마감재가 날아갔다: {back}");
        assert_eq!(back["objects"][0]["data"]["단가"], 12500, "숫자가 안 돌아왔다: {back}");
    }

    #[test]
    fn 긴_한글_의미_정보도_그대로_돌아온다() {
        // 1000 그룹은 255 바이트까지다. 한글은 글자당 3 바이트라 조금만 길어도
        // 쪼개진다 — 바이트로 자르면 글자가 반쪽 나서 깨진 채 돌아온다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "circle", r#"{"center":[0,0],"radius":100}"#));

        let note = "현장 확인 필요 — ".repeat(40);
        let data = serde_json::json!({ "메모": note });
        run(&mut doc, "xdata.set", &format!(r#"{{"handles":["{h}"],"data":{data}}}"#));

        let dxf = doc.to_dxf();
        let mut again = Doc::open(dxf.as_bytes()).unwrap();
        let h2 = again.pick("", 100.0, 0.0, 20.0).unwrap();
        let back = run(&mut again, "xdata", &format!(r#"{{"handles":["{h2}"]}}"#));
        assert_eq!(back["objects"][0]["data"]["메모"], note, "긴 한글이 깨졌다");
    }

    #[test]
    fn 지우고_되돌리면_살아난다() {
        // 되돌리기를 우리가 안 만들어도 되는지가 A 의 핵심이다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "circle", r#"{"center":[0,0],"radius":500}"#));

        run(&mut doc, "erase", &format!(r#"{{"handles":["{h}"]}}"#));
        assert_eq!(doc.pick("", 500.0, 0.0, 20.0), None, "지웠는데 아직 잡힌다");

        run(&mut doc, "u", "{}");
        assert_eq!(doc.pick("", 500.0, 0.0, 20.0).as_deref(), Some(h.as_str()), "되돌렸는데 안 살아났다");

        run(&mut doc, "redo", "{}");
        assert_eq!(doc.pick("", 500.0, 0.0, 20.0), None, "다시 실행했는데 안 지워졌다");
    }

    #[test]
    fn 여러_번_고쳐도_한_단계씩_돌아간다() {
        // tldraw 의 `run()` 묶음에 기대던 걸 엔진이 대신 해주는지.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let h = handle_of(&run(&mut doc, "circle", r#"{"center":[0,0],"radius":100}"#));
        for _ in 0..3 {
            run(&mut doc, "move", &format!(r#"{{"handles":["{h}"],"delta":[1000,0]}}"#));
        }
        // 3000 만큼 갔으니 거기서 잡힌다
        assert_eq!(doc.pick("", 3100.0, 0.0, 20.0).as_deref(), Some(h.as_str()));
        run(&mut doc, "u", "{}");
        assert_eq!(doc.pick("", 2100.0, 0.0, 20.0).as_deref(), Some(h.as_str()), "한 단계만 돌아와야 한다");
    }
}
