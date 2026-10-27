//! Bimove 치수를 엔진 DIMENSION 으로 옮긴다.
//!
//! ── 왜 DIMENSION 인가 ──
//!
//! 지금 화면(`src/shapes/DimensionShape.tsx`)은 치수를 **손으로 그린다**:
//! 연장선 둘, 치수선 하나, 화살표 둘, 그리고 글자 하나. 글자가 뒤집히지 않게
//! `angle > 90 || angle < -90` 이면 180° 를 더하는 보정까지 직접 들어 있다.
//!
//! 엔진에는 그게 전부 있다. `EntityKind::Dimension` 과 `DimStyle` 이 DXF 의
//! DIMASZ·DIMEXE·DIMEXO·DIMGAP·DIMTAD·DIMTIH 를 그대로 들고 있고, 렌더러가
//! 길이를 **제가 재서** 글자를 만들고 (`linear_text`), 글자가 들어갈 자리가
//! 없으면 밖으로 빼고, 각도도 `readable()` 로 뒤집는다. 손으로 옮길 게 없다.
//!
//! 게다가 이쪽이 **살아 있는** 치수다. 두 점만 주면 길이는 렌더 때 다시
//! 재므로, 나중에 엔진에서 점을 끌면 숫자가 따라온다. 선과 글자를 따로 넣으면
//! 그 순간 숫자가 굳는다.
//!
//! ── 숫자를 어떻게 맞추나 ──
//!
//! Bimove 캔버스의 1px 이 몇 mm 인지를 **받아야 한다.** 문서 JSON 의 `scale`
//! (`{pxPerMm, unit}`)이 그것이다.
//!
//! 한때 그 값이 어디에도 저장되지 않았다. tldraw 의 **세션** 상태
//! (`editor.getInstanceState().meta`)에만 있어서, `fromTldrawSnapshot` 이 읽는
//! `document.store` 에 안 들어갔다 — 1:50 으로 맞춰 둔 도면을 다시 열면
//! 1:1 이 됐다는 뜻이다. 지금은 tldraw **문서 레코드**의 meta 로 옮겨서
//! 스냅샷에 실린다 (`lib/scaleConfig.ts`).
//!
//! 그래도 **없을 수 있다.** 옮기기 전에 저장된 도면에는 아무 자리에도 없다.
//! 그때는 `1px = 1mm` 로 두고 `notes` 에 적는다 — 조용히 1 로 두면 2000mm
//! 짜리 벽에 "200mm" 가 찍히고, 그게 틀렸다는 걸 아무도 모른다.
//!
//! 맞추는 방법은 DIMLFAC(`linear_factor`)다. 엔진은 잰 길이에 이걸 곱해서
//! 글자를 만들므로, `1/px_per_mm` 을 주면 px 가 mm 로 바뀐다. 단위 접미사는
//! DIMPOST(`post`)가, 소수점 자리는 DIMDEC(`decimals`)이 맡는다. 그러면
//! `formatLength` 와 같은 문자열이 나온다 — 우리가 문자열을 만들어 박지
//! 않고도.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{DimKind, Dimension, DimStyle, Drawing, Entity, EntityKind};
use cadcraft_geom::{Vec2, Vec3};

/// 치수 하나. 좌표는 **캔버스** 것이다 — y 뒤집기는 이 안에서 한다.
#[derive(Clone, Debug, PartialEq)]
pub struct DimSeg {
    /// 이 치수를 만든 Bimove 도형의 id (`crate::identity`). 치수는 엔티티
    /// 하나지만, 글자를 옮겨도 그게 **이** 치수의 것이어야 한다.
    pub id: String,
    /// 재는 두 점 (캔버스).
    pub a: Vec2,
    pub b: Vec2,
    /// 치수선을 두 점에서 얼마나 띄울지. 부호가 어느 쪽인지를 정한다.
    pub offset: f64,
    pub layer: String,
}

/// 화면이 쓰는 단위.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DimUnit {
    #[default]
    Mm,
    Cm,
    M,
}

impl DimUnit {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "mm" => Some(DimUnit::Mm),
            "cm" => Some(DimUnit::Cm),
            "m" => Some(DimUnit::M),
            _ => None,
        }
    }

    /// `formatLength` 가 쓰는 것과 같은 나눗셈과 소수점 자리.
    ///
    /// mm 는 `Math.round`, cm 는 `toFixed(1)`, m 은 `toFixed(2)` 다.
    fn per_mm(self) -> (f64, u8, &'static str) {
        match self {
            DimUnit::Mm => (1.0, 0, "mm"),
            DimUnit::Cm => (10.0, 1, "cm"),
            DimUnit::M => (1000.0, 2, "m"),
        }
    }
}

/// 캔버스 px 를 실제 길이로 바꾸는 값.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimScale {
    /// 캔버스 1px 이 몇 mm 인가. `0.1` 이면 1px = 1cm 다 (1:10).
    pub px_per_mm: f64,
    pub unit: DimUnit,
}

impl Default for DimScale {
    /// **문서에 스케일이 없을 때의 값.** 1px = 1mm 로 둔다 (`DEFAULT_SCALE`).
    ///
    /// 축척을 문서 레코드에 넣기 전에 저장된 도면이 그렇다.
    ///
    /// 맞는 값이 아니라 **아는 게 없을 때의 값**이다. 부르는 쪽이 이걸 썼다는
    /// 사실을 `notes` 로 알려야 한다.
    fn default() -> Self {
        DimScale { px_per_mm: 1.0, unit: DimUnit::Mm }
    }
}

impl DimScale {
    /// 문서 JSON 의 `scale` 에서 읽는다. 없거나 못 쓰는 값이면 `None`.
    ///
    /// `pxPerMm` 이 0 이나 음수면 **버린다.** 그걸 쓰면 `1/0 = inf` 가
    /// DIMLFAC 으로 들어가 숫자가 통째로 깨진다. 없는 것으로 다루는 편이
    /// 낫다 — 그 쪽은 적어도 `notes` 에 올라온다.
    pub fn from_json(v: Option<&serde_json::Value>) -> Option<Self> {
        let v = v?;
        let px_per_mm = v.get("pxPerMm").and_then(serde_json::Value::as_f64)?;
        if !(px_per_mm > 0.0) || !px_per_mm.is_finite() {
            return None;
        }
        Some(DimScale {
            px_per_mm,
            unit: v
                .get("unit")
                .and_then(serde_json::Value::as_str)
                .and_then(DimUnit::parse)
                .unwrap_or_default(),
        })
    }
}

/// 치수 스타일 이름. 스케일은 문서마다 하나라 스타일도 하나면 된다.
pub const STYLE: &str = "BIMOVE_DIM";

/// 화면의 `color = '#e67e22'`.
///
/// `ByLayer` 로 두지 않는다. 치수는 도면이 아니라 **주석**이라 다른 색으로
/// 읽히는 편이 맞고, 이 주황은 `#222` 같은 흑색 계열이 아니라서 어두운
/// 배경에서도 묻지 않는다 ([[opening.rs]] 가 회색을 버린 이유가 명도였다).
const DIM_COLOR: Rgb = Rgb(0xe6, 0x7e, 0x22);

/// 화면이 쓰는 치수 치장 크기. `DimensionShape.tsx` 의 `arrow`/`ext`/`fontSize`.
const ARROW: f64 = 8.0;
const EXT_BEYOND: f64 = 10.0;
const TEXT_HEIGHT: f64 = 11.0;

/// 길이 0 인 치수는 그릴 것이 없다. 화면도 `len < 2` 면 빈 것을 내놓는다.
const MIN_LEN: f64 = 2.0;

pub fn usable(d: &DimSeg) -> bool {
    d.a.is_finite() && d.b.is_finite() && d.offset.is_finite() && d.a.dist(d.b) >= MIN_LEN
}

/// 이 스케일에 맞는 치수 스타일.
///
/// ISO-25 를 바탕으로 쓴다 — 영(inch) 기준인 `Default` 보다 mm 도면에 가깝다.
/// 그 위에 화면과 같은 크기와, 숫자를 맞추는 세 값을 얹는다.
pub fn style_for(scale: &DimScale) -> DimStyle {
    let (per_mm, decimals, suffix) = scale.unit.per_mm();
    DimStyle {
        name: STYLE.into(),
        // 잰 px 에 이걸 곱하면 화면이 띄우는 숫자가 된다
        linear_factor: 1.0 / (scale.px_per_mm * per_mm),
        decimals,
        // `<>` 가 숫자 자리다. 화면은 단위를 붙여 쓴다 ("1200mm")
        post: format!("<>{suffix}"),
        arrow_size: ARROW,
        ext_extend: EXT_BEYOND,
        // 화면은 연장선을 재는 점에서 **바로** 긋는다 (DIMEXO 0)
        ext_offset: 0.0,
        text_height: TEXT_HEIGHT,
        text_gap: TEXT_HEIGHT * 0.25,
        // 화면도 글자를 치수선 위에 얹는다 (`midY - 6`)
        text_above: 1,
        // 비스듬한 치수의 글자는 치수선을 따라 눕는다 — 화면의 `textAngle` 과 같다
        text_inside_horizontal: false,
        text_outside_horizontal: false,
        dim_line_color: Color::True(DIM_COLOR),
        ext_line_color: Color::True(DIM_COLOR),
        text_color: Color::True(DIM_COLOR),
        // 0 을 지우면 "0mm" 가 빈 글자가 된다. 지우지 않는다.
        zero_suppression: 0,
        ..DimStyle::iso25()
    }
}

/// 스타일을 도면에 한 번 넣는다.
fn register(drawing: &mut Drawing, scale: &DimScale) {
    let def = style_for(scale);
    match drawing.dim_styles.iter_mut().find(|s| s.name == def.name) {
        // 같은 이름이 이미 있으면 **갈아끼운다.** 남겨 두면 옛 스케일로 숫자가 나온다.
        Some(old) => *old = def,
        None => drawing.dim_styles.push(def),
    }
}

/// 치수 하나를 엔티티로.
///
/// 캔버스에서 치수선 자리를 잡고 두 점의 y 를 같이 뒤집는다 — 벽·문창과 같은
/// 규칙이다. DXF 의 정의점 이름대로 `p13`/`p14` 가 재는 두 점이고 `defpt` 는
/// **치수선 위의 한 점**이다. 길이는 렌더러가 `p13`/`p14` 에서 다시 재므로
/// 여기서 굳히지 않는다.
fn entity_for(d: &DimSeg) -> Dimension {
    let (dx, dy) = (d.b.x - d.a.x, d.b.y - d.a.y);
    let len = dx.hypot(dy);
    // 화면의 `nx = -y2/len, ny = x2/len` — 캔버스에서의 왼쪽 법선
    let (nx, ny) = (-dy / len, dx / len);
    let (ox, oy) = (nx * d.offset, ny * d.offset);

    // 캔버스 → 엔진: y 만 뒤집는다
    let at = |x: f64, y: f64| Vec3::new(x, -y, 0.0);
    let on_line1 = (d.a.x + ox, d.a.y + oy);
    let on_line2 = (d.b.x + ox, d.b.y + oy);

    Dimension {
        kind: DimKind::Aligned,
        // 치수선 위의 점
        defpt: at(on_line1.0, on_line1.1),
        // 글자 자리의 기본값. `user_text_pos` 가 false 라 렌더러가 다시 잡는다
        text_mid: at((on_line1.0 + on_line2.0) / 2.0, (on_line1.1 + on_line2.1) / 2.0),
        p13: at(d.a.x, d.a.y),
        p14: at(d.b.x, d.b.y),
        p15: Vec3::ZERO,
        p16: Vec3::ZERO,
        // 빈 문자열 = **잰 값을 쓴다.** 문자열을 박으면 거기서 숫자가 굳는다
        text: String::new(),
        style: STYLE.into(),
        measurement: len,
        text_rotation: 0.0,
        user_text_pos: false,
        block: None,
        overrides: serde_json::Map::new(),
        assoc: Vec::new(),
        marks: Default::default(),
    }
}

/// 치수를 도면에 넣는다.
///
/// @returns 넣은 개수. 못 쓰는 것은 세지 않는다 — 부르는 쪽이 그 차이를 알려야 한다.
pub fn add_dims(drawing: &mut Drawing, dims: &[DimSeg], scale: &DimScale) -> usize {
    if !dims.iter().any(usable) {
        return 0;
    }
    register(drawing, scale);

    let mut n = 0;
    for d in dims.iter().filter(|d| usable(d)) {
        let h = drawing.new_handle();
        let mut e = Entity::new(h, EntityKind::Dimension(entity_for(d)));
        e.common.layer = d.layer.clone();
        // 색은 스타일이 쥐고 있다 (DIMCLRD/E/T). 엔티티 색까지 박으면 둘이 싸운다
        e.common.color = Color::ByLayer;
        let from = drawing.model.len();
        drawing.model.push(e);
        crate::identity::stamp_one(drawing, from, crate::identity::DIM, &d.id);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn seg(ax: f64, ay: f64, bx: f64, by: f64, offset: f64) -> DimSeg {
        DimSeg { id: String::new(), a: Vec2::new(ax, ay), b: Vec2::new(bx, by), offset, layer: "A-DIM".into() }
    }

    fn dims(d: &Drawing) -> Vec<Dimension> {
        d.model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Dimension(x) => Some(x.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn 치수를_하나_넣는다() {
        let mut d = Drawing::new_metric();
        assert_eq!(add_dims(&mut d, &[seg(0.0, 0.0, 200.0, 0.0, 60.0)], &DimScale::default()), 1);
        let x = &dims(&d)[0];
        assert_eq!(x.kind, DimKind::Aligned);
        assert_eq!(x.style, STYLE);
        // 빈 글자 = 잰 값을 쓴다. 문자열을 박으면 숫자가 굳는다
        assert!(x.text.is_empty(), "글자를 박아 뒀다: {:?}", x.text);
        assert!(!x.user_text_pos, "글자 자리를 렌더러에 맡겨야 한다");
    }

    #[test]
    fn 재는_두_점이_p13_p14_다() {
        let mut d = Drawing::new_metric();
        add_dims(&mut d, &[seg(10.0, 20.0, 210.0, 20.0, 60.0)], &DimScale::default());
        let x = &dims(&d)[0];
        // y 가 뒤집힌다
        assert_eq!(x.p13.xy(), Vec2::new(10.0, -20.0));
        assert_eq!(x.p14.xy(), Vec2::new(210.0, -20.0));
    }

    #[test]
    fn 치수선이_offset_만큼_떨어진다() {
        // 가로 치수에 offset 60 → 캔버스에서 n = (0, 1) 이라 y 가 +60,
        // 엔진에서는 -60 이다. 재는 점(-20)보다 **더 내려간** 자리다.
        let mut d = Drawing::new_metric();
        add_dims(&mut d, &[seg(0.0, 20.0, 200.0, 20.0, 60.0)], &DimScale::default());
        let x = &dims(&d)[0];
        assert_eq!(x.defpt.xy().y, -80.0, "치수선 {:?}", x.defpt);
        // 치수선은 재는 선과 나란하다
        assert_eq!(x.defpt.xy().x, 0.0);
    }

    #[test]
    fn offset_부호가_치수선_쪽을_바꾼다() {
        let mut up = Drawing::new_metric();
        add_dims(&mut up, &[seg(0.0, 0.0, 200.0, 0.0, 60.0)], &DimScale::default());
        let mut down = Drawing::new_metric();
        add_dims(&mut down, &[seg(0.0, 0.0, 200.0, 0.0, -60.0)], &DimScale::default());
        assert!(
            dims(&up)[0].defpt.xy().y * dims(&down)[0].defpt.xy().y < 0.0,
            "{:?} / {:?}",
            dims(&up)[0].defpt,
            dims(&down)[0].defpt
        );
    }

    #[test]
    fn 비스듬한_치수도_수직으로_떨어진다() {
        // 치수선은 재는 선과 나란하고, 거리가 정확히 offset 이어야 한다
        let mut d = Drawing::new_metric();
        add_dims(&mut d, &[seg(0.0, 0.0, 120.0, 160.0, 50.0)], &DimScale::default());
        let x = &dims(&d)[0];
        let (p1, p2) = (x.p13.xy(), x.p14.xy());
        let u = (p2 - p1).normalized();
        let v = x.defpt.xy() - p1;
        // 나란한 성분을 뺀 나머지가 수직 거리다
        let perp = (v - u * v.dot(u)).len();
        assert!((perp - 50.0).abs() < 1e-9, "수직 거리 {perp}");
    }

    #[test]
    fn mm_는_정수로_떨어지고_단위가_붙는다() {
        // 1:10 (1px = 1cm = 10mm) 이면 200px 은 2000mm 다
        let s = DimScale { px_per_mm: 0.1, unit: DimUnit::Mm };
        let st = style_for(&s);
        assert_eq!(st.linear_factor, 10.0);
        assert_eq!(st.decimals, 0);
        assert_eq!(st.post, "<>mm");
    }

    #[test]
    fn cm_와_m_도_화면과_같은_자리까지_쓴다() {
        let cm = style_for(&DimScale { px_per_mm: 0.1, unit: DimUnit::Cm });
        assert_eq!(cm.decimals, 1);
        assert_eq!(cm.post, "<>cm");
        // 200px = 2000mm = 200.0cm
        assert!((cm.linear_factor * 200.0 - 200.0).abs() < 1e-9, "{}", cm.linear_factor);

        let m = style_for(&DimScale { px_per_mm: 0.1, unit: DimUnit::M });
        assert_eq!(m.decimals, 2);
        assert_eq!(m.post, "<>m");
        // 200px = 2000mm = 2.00m
        assert!((m.linear_factor * 200.0 - 2.0).abs() < 1e-9, "{}", m.linear_factor);
    }

    #[test]
    fn 스케일을_안_주면_1px_1mm_다() {
        let s = DimScale::default();
        assert_eq!(s.px_per_mm, 1.0);
        assert_eq!(s.unit, DimUnit::Mm);
        assert_eq!(style_for(&s).linear_factor, 1.0);
    }

    #[test]
    fn 문서의_scale_을_읽는다() {
        let v = json!({"pxPerMm": 0.02, "unit": "m"});
        let s = DimScale::from_json(Some(&v)).expect("못 읽었다");
        assert_eq!(s.px_per_mm, 0.02);
        assert_eq!(s.unit, DimUnit::M);
    }

    #[test]
    fn 못_쓰는_scale_은_안_받는다() {
        // 0 이나 음수를 받으면 1/0 = inf 가 linear_factor 로 들어가 숫자가 깨진다
        for bad in [json!({"pxPerMm": 0}), json!({"pxPerMm": -1}), json!({"unit": "mm"}), json!({})] {
            assert!(DimScale::from_json(Some(&bad)).is_none(), "{bad} 를 받았다");
        }
        assert!(DimScale::from_json(None).is_none());
        // 모르는 단위는 mm 로 떨어뜨린다 — pxPerMm 만 맞으면 쓸 수 있다
        let v = json!({"pxPerMm": 0.1, "unit": "척"});
        assert_eq!(DimScale::from_json(Some(&v)).expect("버렸다").unit, DimUnit::Mm);
    }

    #[test]
    fn 스타일을_갈아끼운다() {
        // 같은 이름을 쌓아 두면 먼저 것이 이기고 옛 스케일로 숫자가 나온다
        let mut d = Drawing::new_metric();
        let one = seg(0.0, 0.0, 200.0, 0.0, 60.0);
        add_dims(&mut d, &[one.clone()], &DimScale { px_per_mm: 1.0, unit: DimUnit::Mm });
        add_dims(&mut d, &[one], &DimScale { px_per_mm: 0.1, unit: DimUnit::Mm });
        let found: Vec<&DimStyle> = d.dim_styles.iter().filter(|s| s.name == STYLE).collect();
        assert_eq!(found.len(), 1, "스타일이 {}개다", found.len());
        assert_eq!(found[0].linear_factor, 10.0, "옛 스케일이 남았다");
    }

    #[test]
    fn 길이_0_과_nan_은_버린다() {
        let mut d = Drawing::new_metric();
        let bad = [
            seg(0.0, 0.0, 0.0, 0.0, 60.0),
            seg(0.0, 0.0, 1.0, 0.0, 60.0),
            seg(f64::NAN, 0.0, 200.0, 0.0, 60.0),
            seg(0.0, 0.0, 200.0, 0.0, f64::NAN),
        ];
        assert_eq!(add_dims(&mut d, &bad, &DimScale::default()), 0);
        assert!(d.model.is_empty());
        // 하나도 안 넣었으면 스타일도 만들지 않는다
        assert!(d.dim_style(STYLE).is_none());
    }

    #[test]
    fn 레이어가_붙고_색은_스타일이_쥔다() {
        let mut d = Drawing::new_metric();
        add_dims(&mut d, &[seg(0.0, 0.0, 200.0, 0.0, 60.0)], &DimScale::default());
        let e = d.model.iter().next().expect("엔티티가 없다");
        assert_eq!(e.common.layer, "A-DIM");
        // 엔티티 색까지 박으면 DIMCLRD/E/T 와 싸운다
        assert_eq!(e.common.color, Color::ByLayer);
        let st = d.dim_style(STYLE).expect("스타일이 없다");
        assert_eq!(st.text_color, Color::True(DIM_COLOR));
    }

    #[test]
    fn 화면과_같은_치장_크기를_쓴다() {
        let st = style_for(&DimScale::default());
        assert_eq!(st.arrow_size, 8.0);
        assert_eq!(st.ext_extend, 10.0);
        assert_eq!(st.ext_offset, 0.0, "화면은 재는 점에서 바로 긋는다");
        assert_eq!(st.text_height, 11.0);
        assert_eq!(st.text_above, 1, "글자는 치수선 위에 얹힌다");
    }

    /// 그려진 프리미티브 수. 글자는 글립마다 하나씩 나오므로 숫자가 찍히면 늘어난다.
    fn prims(d: &Drawing) -> usize {
        let opts = cadcraft_render::Options::default();
        cadcraft_render::build(d, &cadcraft_doc::Space::Model, &opts).prims.len()
    }

    #[test]
    fn 숫자가_실제로_그려진다() {
        // 선과 화살표만 나오고 글자가 빠져도 "치수가 그려졌다" 로 보인다.
        // DXF 는 빈 글자를 "잰 값을 쓴다", 공백 하나를 "숨긴다" 로 읽으므로
        // 그 둘을 견주면 글자가 정말 나오는지 보인다.
        let one = seg(0.0, 0.0, 200.0, 0.0, 60.0);
        let mut with = Drawing::new_metric();
        add_dims(&mut with, &[one.clone()], &DimScale::default());

        let mut without = Drawing::new_metric();
        register(&mut without, &DimScale::default());
        let mut hidden = entity_for(&one);
        // DXF 는 공백 하나를 "글자를 숨긴다" 로 읽는다
        hidden.text = " ".into();
        let h = without.new_handle();
        without.model.push(Entity::new(h, EntityKind::Dimension(hidden)));

        let (a, b) = (prims(&with), prims(&without));
        assert!(a > b, "글자가 안 그려졌다 — 있을 때 {a}개, 숨겼을 때 {b}개");
    }

    #[test]
    fn 여러_개를_넣는다() {
        let mut d = Drawing::new_metric();
        let n = add_dims(
            &mut d,
            &[seg(0.0, 0.0, 200.0, 0.0, 60.0), seg(0.0, 0.0, 0.0, 150.0, -40.0)],
            &DimScale::default(),
        );
        assert_eq!(n, 2);
        assert_eq!(dims(&d).len(), 2);
    }

    #[test]
    fn 치수에_정체성이_붙는다() {
        let mut d = Drawing::new_metric();
        let mut x = seg(0.0, 0.0, 200.0, 0.0, 60.0);
        x.id = "shape:dim1".into();
        add_dims(&mut d, &[x], &DimScale::default());

        let g = crate::identity::of(d.model.iter().next().expect("치수가 없다"))
            .expect("치수에 정체성이 없다");
        assert_eq!(g.kind, crate::identity::DIM);
        assert_eq!(g.ids, vec!["shape:dim1"]);
    }

    #[test]
    fn 출처_없는_치수에는_안_붙인다() {
        let mut d = Drawing::new_metric();
        add_dims(&mut d, &[seg(0.0, 0.0, 200.0, 0.0, 60.0)], &DimScale::default());
        assert!(d.model.iter().all(|e| crate::identity::of(e).is_none()));
    }
}
