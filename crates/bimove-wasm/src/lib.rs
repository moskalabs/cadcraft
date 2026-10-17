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

use cadcraft_doc::{Drawing, Space};
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
    prims: Vec<i32>,
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
    fn raster(
        &mut self, layout: &str, cx: f64, cy: f64, scale: f64, w: u32, h: u32, bg: u32,
    ) -> Option<Raster> {
        // 곡선을 선분으로 쪼갤 오차는 "화면 반 픽셀". 줌이 바뀌면 같이 바뀐다.
        let tol = 0.5 / scale.max(1e-12);
        let reusable = self.cache.as_ref().is_some_and(|(l, t, _)| {
            l == layout && *t / TOL_SLACK <= tol && tol <= *t * TOL_SLACK
        });
        if !reusable {
            let space = if layout.is_empty() {
                Space::Model
            } else {
                Space::Paper(layout.to_string())
            };
            let opts = Options { tolerance: tol, ..Options::default() };
            let dl = cadcraft_render::build(&self.drawing, &space, &opts);
            self.cache = Some((layout.to_string(), tol, dl));
        }
        let dl = &self.cache.as_ref()?.2;
        let view = cadcraft_render::raster::View {
            center: cadcraft_geom::Vec2 { x: cx, y: cy },
            scale,
            width: w,
            height: h,
        };
        let ro = cadcraft_render::raster::RasterOptions {
            background: cadcraft_color::Rgb((bg >> 16) as u8, (bg >> 8) as u8, bg as u8),
            ..Default::default()
        };
        let pm = cadcraft_render::raster::render(dl, &view, &ro)?;

        // tiny-skia 는 **알파 곱해진(premultiplied)** 픽셀을 준다. 캔버스
        // ImageData 는 곱하지 않은 값을 원한다 — 그냥 넘기면 반투명한 것이
        // 어둡게 나온다. 여기서 되돌린다.
        let mut rgba = Vec::with_capacity((w as usize) * (h as usize) * 4);
        for p in pm.pixels() {
            let c = p.demultiply();
            rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
        Some(Raster { rgba, w, h })
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
            &self.drawing,
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
        cadcraft_engine::select::select_window(&self.drawing, &Self::space_of(layout), bx, crossing)
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
            &self.drawing,
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
pub struct Doc {
    drawing: Drawing,
    /// 마지막으로 만든 DisplayList 와 그때의 (공간, tolerance).
    ///
    /// **이게 성능의 전부다.** 매 프레임 build 하면 45MB 도면에서 한 장이
    /// 240ms 인데, 그중 래스터는 15ms 뿐이고 나머지가 build 다. 팬은 카메라만
    /// 움직이므로 같은 목록을 다시 쓰면 된다.
    cache: Option<(String, f64, DisplayList)>,
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
        Ok(Doc { drawing, cache: None })
    }

    /// 배치(종이공간) 탭 이름들. 모형 탭은 여기 없다 — `Space::Model` 로 따로 연다.
    fn layout_names(&self) -> Vec<String> {
        self.drawing.layouts.iter().map(|l| l.name.clone()).collect()
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
        Frame::from_list(&cadcraft_render::build(&self.drawing, &space, &opts))
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
    #[wasm_bindgen(js_name = renderPixels)]
    #[allow(clippy::too_many_arguments)]
    pub fn render_pixels(
        &mut self, layout: &str, cx: f64, cy: f64, scale: f64, w: u32, h: u32, bg: u32,
    ) -> Option<BimoveRaster> {
        self.0.raster(layout, cx, cy, scale, w, h, bg).map(BimoveRaster)
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
        let r = doc.raster("", 50.0, 25.0, 1.5, w, h, 0xFFFFFF).expect("래스터 실패");
        assert_eq!(r.w, w);
        assert_eq!(r.h, h);
        assert_eq!(r.rgba.len(), (w as usize) * (h as usize) * 4, "RGBA 길이가 안 맞는다");

        let bg = r.rgba.chunks_exact(4).filter(|p| p[0] == 255 && p[1] == 255 && p[2] == 255).count();
        let ink = (w as usize) * (h as usize) - bg;
        assert!(ink > 20, "배경이 아닌 픽셀이 {ink}개뿐 — 선이 안 찍혔다");
        assert!(bg > 0, "전부 잉크다 — 배경색이 안 먹었다");
    }

    #[test]
    fn 알파는_곱해지지_않은_채로_나온다() {
        // tiny-skia 는 알파가 곱해진 픽셀을 준다. 그대로 넘기면 캔버스에서
        // 색이 어두워진다. 불투명 배경이니 알파가 전부 255 여야 한다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        let r = doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF).unwrap();
        assert!(r.rgba.chunks_exact(4).all(|p| p[3] == 255), "알파가 255 가 아닌 픽셀이 있다");
    }

    #[test]
    fn 팬은_목록을_다시_만들지_않는다() {
        // 카메라만 움직이면 같은 DisplayList 를 쓴다. 이게 깨지면 45MB 도면에서
        // 한 프레임이 15ms 에서 240ms 로 돌아간다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF).unwrap();
        let built = doc.cache.as_ref().map(|(_, t, dl)| (*t, dl.prims.len())).unwrap();
        doc.raster("", 999.0, -999.0, 1.5, 64, 64, 0xFFFFFF).unwrap();
        let after = doc.cache.as_ref().map(|(_, t, dl)| (*t, dl.prims.len())).unwrap();
        assert_eq!(built, after, "팬만 했는데 목록이 다시 만들어졌다");
    }

    #[test]
    fn 줌이_크게_바뀌면_다시_만든다() {
        // tolerance 는 줌에 반비례한다. 2배 안쪽은 그대로 쓰고, 넘으면 다시
        // 만들어야 곡선이 거칠어지지 않는다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.0, 64, 64, 0xFFFFFF).unwrap();
        let t0 = doc.cache.as_ref().unwrap().1;

        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF).unwrap();
        assert_eq!(doc.cache.as_ref().unwrap().1, t0, "1.5배 줌은 다시 만들 일이 아니다");

        doc.raster("", 50.0, 25.0, 10.0, 64, 64, 0xFFFFFF).unwrap();
        assert_ne!(doc.cache.as_ref().unwrap().1, t0, "10배 줌인데 그대로 썼다");
    }

    #[test]
    fn 공간이_바뀌면_다시_만든다() {
        // 모형 ↔ 배치는 아예 다른 내용이다. 캐시를 그대로 쓰면 엉뚱한 걸 그린다.
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        doc.raster("", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF).unwrap();
        assert_eq!(doc.cache.as_ref().unwrap().0, "");
        doc.raster("배치1", 50.0, 25.0, 1.5, 64, 64, 0xFFFFFF).unwrap();
        assert_eq!(doc.cache.as_ref().unwrap().0, "배치1", "공간이 바뀌었는데 캐시가 그대로다");
    }

    #[test]
    fn 크기가_0_이면_실패를_돌려준다() {
        let mut doc = Doc::open(&one_line_dxf()).unwrap();
        assert!(doc.raster("", 0.0, 0.0, 1.0, 0, 100, 0xFFFFFF).is_none());
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

    #[test]
    fn 깨진_입력은_에러로_돌려준다() {
        // 조용히 빈 도면을 주면 "열렸는데 아무것도 없네" 가 되어 원인을 못 찾는다.
        assert!(Doc::open(b"this is not a dxf at all").is_err());
    }
}
