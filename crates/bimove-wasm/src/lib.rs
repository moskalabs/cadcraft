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
}

impl Doc {
    fn open(bytes: &[u8]) -> Result<Doc, String> {
        let drawing = cadcraft_io::read_dxf(bytes).map_err(|e| e.to_string())?;
        Ok(Doc { drawing })
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
    fn 깨진_입력은_에러로_돌려준다() {
        // 조용히 빈 도면을 주면 "열렸는데 아무것도 없네" 가 되어 원인을 못 찾는다.
        assert!(Doc::open(b"this is not a dxf at all").is_err());
    }
}
