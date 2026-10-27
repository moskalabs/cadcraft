//! Bimove 공간(zone)을 엔진 엔티티로 옮긴다.
//!
//! 공간은 사용자가 그린 닫힌 폴리곤 + 이름 + 면적이다. 벽이 아니라 **주석**이고,
//! 마감재 물량을 거는 자리다.
//!
//! ── 반투명이어야 한다 ──
//!
//! 공간은 벽·가구 **위에** 깔린다 (`lib/zoneStyle.ts`). 진하게 칠하면 그 밑이
//! 안 보여서 어디를 집었는지도, 그 안에 뭐가 있는지도 못 본다. 그래서 알파가
//! 낮다 — 면 `0.10`, 테두리 `0.55`.
//!
//! 면은 0.16 에서 내렸다 (2026-10-27). **이게 바닥이다** — 엔진 투명도는 90%
//! 까지만 받으므로 `0.10` 이 그대로 90% 로 떨어진다. 화면 쪽(`zoneStyle.ts`)을
//! 더 내리면 여기가 못 따라가서 두 그림이 갈린다.
//!
//! 엔진에는 그 자리가 있다: `Common::transparency`. 다만 렌더 옵션
//! `Options::transparency` 가 **꺼져 있으면 무시된다** (AutoCAD 의
//! TRANSPARENCYDISPLAY 를 따르는 자리다). 꺼 두면 공간이 **불투명한 회색
//! 덩어리**가 되어 평면도를 덮는다. 그래서 `lib.rs` 의 렌더 옵션에서 켠다.
//!
//! ── 차례가 맨 뒤다 ──
//!
//! 반투명이므로 **밑에 뭐가 깔려 있는지가 결과에 들어온다.** 벽·문창보다
//! 나중에 넣어야 화면과 같은 그림이 된다. `drawing_from_bimove` 가 맨 마지막에
//! 부른다.
//!
//! ── 면적은 **그릴 때 굳은 값**이다 ──
//!
//! `props.areaM2` 는 공간을 그린 순간의 축척으로 계산해 넣어 둔 값이다
//! (`ZoneDrawOverlay`). 즉 나중에 축척을 바꿔도 **안 따라온다.** 치수와
//! 다른 점이 그거다 — 치수는 두 점만 들고 있어서 렌더 때 다시 재지만, 공간은
//! 숫자가 이미 박혀 있다.
//!
//! 여기서 폴리곤으로 **다시 재지 않는다.** 화면이 띄우는 숫자와 달라지면
//! "둘 중 어느 게 맞나" 를 아무도 못 가린다. 고치려면 Bimove 쪽에서 축척이
//! 바뀔 때 `areaM2` 를 다시 계산해 넣는 게 맞고, 그건 이 파일의 일이 아니다.
//!
//! ── 이름만 색을 남긴다 ──
//!
//! 면을 전부 회색으로 하면 공간이 여럿일 때 구분이 안 된다. `props.color` 는
//! 그래서 **글자**에 쓴다 (면이 아니다). 같은 결정이 `zoneStyle.ts` 에 적혀
//! 있다.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{
    Drawing, Entity, EntityKind, Hatch, HatchLoop, LwPolyline, MText, Transparency,
};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};

use crate::jsnum::to_fixed;

/// 공간 하나. 좌표는 **캔버스** 것이다 — y 뒤집기는 이 안에서 한다.
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// 이 공간을 만든 Bimove 도형의 id. 면·테두리·이름·면적 글자가 이걸로
    /// **한 덩어리**가 된다 (`crate::identity`). 빈 문자열이면 심지 않는다.
    pub id: String,
    /// 닫힌 폴리곤. 첫 점을 끝에 되풀이하지 않는다.
    pub points: Vec<Vec2>,
    /// 공간 이름. 비어 있으면 "미지정" 으로 띄운다 (화면과 같다).
    pub label: String,
    /// 그릴 때 굳은 면적. `0` 이하면 안 띄운다.
    pub area_m2: f64,
    /// **이름 글자** 색. 면 색이 아니다.
    pub color: Rgb,
    pub layer: String,
}

/// 테두리에 쓰는 점선. 화면의 `strokeDasharray="8 4"` 와 같은 길이다.
///
/// 점선인 건 "벽이 아니라 주석" 이라는 신호다 (`zoneStyle.ts`).
pub const DASH: &str = "BIMOVE_ZONE";

/// 면과 테두리 색 — `rgba(100,108,120, …)` 의 RGB 부분.
const WASH: Rgb = Rgb(100, 108, 120);

/// 면적 글자 — 이름보다 한 단계 여리게 (`ZONE_AREA_TEXT`).
const AREA_INK: Rgb = Rgb(0x5a, 0x62, 0x70);

/// 화면의 `fontSize`.
const LABEL_HEIGHT: f64 = 14.0;
const AREA_HEIGHT: f64 = 11.0;

/// 가운데를 기준으로 글자를 얼마나 올리고 내릴지 (화면의 `c.y ± 6 / 12`).
const LABEL_RISE: f64 = 6.0;
const AREA_DROP: f64 = 12.0;

/// 알파를 DXF 투명도(%)로. `0.10` → 90%.
///
/// 엔진은 90% 까지만 받는다 (그 위는 아예 안 보이는 것과 같아서 클램프된다).
fn transparency_of(alpha: f64) -> Transparency {
    let pct = ((1.0 - alpha.clamp(0.0, 1.0)) * 100.0).round();
    Transparency::Percent(pct.clamp(0.0, 90.0) as u8)
}

/// MTEXT 붙임점 5 = 가운데-가운데. 화면의 `textAnchor="middle"` +
/// `dominantBaseline="middle"` 과 같은 자리다.
const ATTACH_MIDDLE_CENTER: u8 = 5;

/// 폴리곤이 될 수 있나. 점이 셋 미만이면 면이 안 된다 (화면도 그때 안 그린다).
pub fn usable(z: &Zone) -> bool {
    z.points.len() >= 3 && z.points.iter().all(|p| p.is_finite())
}

/// 점선 선종을 도면에 넣어 둔다. 없으면 테두리가 실선으로 나온다.
pub fn register_linetypes(drawing: &mut Drawing) {
    if drawing.linetype(DASH).is_none() {
        drawing
            .linetypes
            .push(cadcraft_doc::Linetype::simple(DASH, "공간 테두리", &[8.0, -4.0]));
    }
}

/// 꼭짓점 평균. 화면의 `centroid` 와 **같은 식**이다.
///
/// 무게중심(면적 가중)이 아니다. 둘은 다른 점이고, 화면이 쓰는 쪽을 따라가야
/// 글자가 같은 자리에 선다 — 오목한 공간에서 눈에 띄게 벌어진다.
fn label_anchor(points: &[Vec2]) -> Vec2 {
    let n = points.len() as f64;
    let sum = points.iter().fold(Vec2::ZERO, |a, p| a + *p);
    Vec2::new(sum.x / n, sum.y / n)
}

fn text(
    drawing: &mut Drawing, z: &Zone, at: Vec2, height: f64, rgb: Rgb, contents: &str,
) {
    let h = drawing.new_handle();
    let mut e = Entity::new(
        h,
        EntityKind::MText(MText {
            insert: Vec3::new(at.x, at.y, 0.0),
            height,
            width: 0.0,
            attach: ATTACH_MIDDLE_CENTER,
            rotation: 0.0,
            style: "Standard".into(),
            contents: contents.to_string(),
            line_spacing: 1.0,
            line_spacing_exact: false,
            columns: None,
        }),
    );
    e.common.layer = z.layer.clone();
    e.common.color = Color::True(rgb);
    drawing.model.push(e);
}

fn one(drawing: &mut Drawing, z: &Zone) {
    // 캔버스 → 엔진: y 만 뒤집는다
    let at = |p: Vec2| Vec2::new(p.x, -p.y);
    let pts: Vec<Vec2> = z.points.iter().map(|p| at(*p)).collect();

    // 1. 면 — 반투명 회색. 테두리보다 **먼저** 와야 테두리를 덮지 않는다
    let h = drawing.new_handle();
    let mut fill = Entity::new(
        h,
        EntityKind::Hatch(Hatch {
            pattern: "SOLID".into(),
            solid: true,
            loops: vec![HatchLoop {
                vertices: pts.iter().map(|p| PolyVertex::new(*p)).collect(),
                outer: true,
                sources: Vec::new(),
            }],
            scale: 1.0,
            angle: 0.0,
            associative: false,
            style: 0,
            elevation: 0.0,
            gradient: None,
            origin: Vec2::ZERO,
            background: None,
            pattern_lines: Vec::new(),
        }),
    );
    fill.common.layer = z.layer.clone();
    fill.common.color = Color::True(WASH);
    fill.common.transparency = transparency_of(0.10);
    drawing.model.push(fill);

    // 2. 테두리 — 점선. 닫은 폴리라인이라 첫 점을 끝에 되풀이하지 않는다
    let h = drawing.new_handle();
    let mut edge = Entity::new(
        h,
        EntityKind::LwPolyline(LwPolyline {
            vertices: pts.iter().map(|p| PolyVertex::new(*p)).collect(),
            closed: true,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        }),
    );
    edge.common.layer = z.layer.clone();
    edge.common.color = Color::True(WASH);
    edge.common.linetype = DASH.to_string();
    edge.common.transparency = transparency_of(0.55);
    drawing.model.push(edge);

    // 3. 이름 — 가운데 위. 색은 공간마다 다르다 (그래서 구분이 된다)
    let c = at(label_anchor(&z.points));
    let label = if z.label.trim().is_empty() { "미지정" } else { z.label.trim() };
    text(drawing, z, Vec2::new(c.x, c.y + LABEL_RISE), LABEL_HEIGHT, z.color, label);

    // 4. 면적 — 가운데 아래. 0 이면 안 띄운다 (화면과 같다)
    if z.area_m2 > 0.0 {
        let s = format!("{}m²", to_fixed(z.area_m2, 1));
        text(drawing, z, Vec2::new(c.x, c.y - AREA_DROP), AREA_HEIGHT, AREA_INK, &s);
    }
}

/// 공간을 도면에 넣는다. **벽·문창을 넣은 뒤에** 불러야 한다 (모듈 주석 참고).
///
/// @returns 넣은 개수. 못 쓰는 것은 세지 않는다 — 부르는 쪽이 그 차이를 알려야 한다.
pub fn add_zones(drawing: &mut Drawing, zones: &[Zone]) -> usize {
    if !zones.iter().any(usable) {
        return 0;
    }
    register_linetypes(drawing);

    let mut n = 0;
    for z in zones.iter().filter(|z| usable(z)) {
        // 하나가 면 + 테두리 + 글자 둘로 흩어진다. 넣은 범위를 한 번에 찍어
        // 같은 공간으로 묶는다.
        let from = drawing.model.len();
        one(drawing, z);
        crate::identity::stamp_one(drawing, from, crate::identity::ZONE, &z.id);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(label: &str, area: f64) -> Zone {
        Zone {
            id: String::new(),
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(100.0, 0.0),
                Vec2::new(100.0, 80.0),
                Vec2::new(0.0, 80.0),
            ],
            label: label.into(),
            area_m2: area,
            color: Rgb(0x1a, 0x73, 0xe8),
            layer: "A-ZONE".into(),
        }
    }

    fn kinds(d: &Drawing) -> Vec<&'static str> {
        d.model
            .iter()
            .map(|e| match e.kind {
                EntityKind::Hatch(_) => "면",
                EntityKind::LwPolyline(_) => "테두리",
                EntityKind::MText(_) => "글자",
                _ => "기타",
            })
            .collect()
    }

    fn texts(d: &Drawing) -> Vec<(String, f64, Color, Vec2)> {
        d.model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::MText(t) => {
                    Some((t.contents.clone(), t.height, e.common.color, t.insert.xy()))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn 면_테두리_이름_면적_차례로_넣는다() {
        let mut d = Drawing::new_metric();
        assert_eq!(add_zones(&mut d, &[square("거실", 8.0)]), 1);
        // 면이 **먼저** 와야 테두리와 글자를 덮지 않는다
        assert_eq!(kinds(&d), vec!["면", "테두리", "글자", "글자"]);
    }

    #[test]
    fn 면적이_0_이면_안_띄운다() {
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 0.0)]);
        assert_eq!(kinds(&d), vec!["면", "테두리", "글자"]);
        assert_eq!(texts(&d)[0].0, "거실");
    }

    #[test]
    fn 면은_반투명이다() {
        // 불투명하면 평면도를 덮어서 그 밑에 뭐가 있는지 못 본다
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 8.0)]);
        let fill = d.model.iter().next().expect("엔티티가 없다");
        assert_eq!(fill.common.transparency, Transparency::Percent(90));
        let edge = d.model.iter().nth(1).expect("테두리가 없다");
        assert_eq!(edge.common.transparency, Transparency::Percent(45));
    }

    #[test]
    fn 알파를_투명도로_바꾼다() {
        assert_eq!(transparency_of(1.0), Transparency::Percent(0));
        assert_eq!(transparency_of(0.5), Transparency::Percent(50));
        // 엔진은 90% 까지만 받는다 — 그 위는 안 보이는 것과 같다
        assert_eq!(transparency_of(0.0), Transparency::Percent(90));
        assert_eq!(transparency_of(0.05), Transparency::Percent(90));
    }

    #[test]
    fn y_가_뒤집힌다() {
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 8.0)]);
        let ys: Vec<f64> = match &d.model.iter().next().expect("면").kind {
            EntityKind::Hatch(h) => h.loops[0].vertices.iter().map(|v| v.p.y).collect(),
            other => panic!("면이 아니다: {other:?}"),
        };
        // 캔버스 y 0..80 → 엔진 -80..0
        assert_eq!(ys.iter().cloned().fold(f64::MAX, f64::min), -80.0);
        assert_eq!(ys.iter().cloned().fold(f64::MIN, f64::max), 0.0);
    }

    #[test]
    fn 테두리는_닫히고_점선이다() {
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 8.0)]);
        let edge = d.model.iter().nth(1).expect("테두리");
        match &edge.kind {
            // 닫았으므로 첫 점을 끝에 되풀이하지 않는다 — 네 점이면 네 점이다
            EntityKind::LwPolyline(p) => {
                assert!(p.closed, "테두리가 안 닫혔다");
                assert_eq!(p.vertices.len(), 4);
            }
            other => panic!("테두리가 아니다: {other:?}"),
        }
        assert_eq!(edge.common.linetype, DASH);
        let lt = d.linetype(DASH).expect("점선이 없다");
        assert!(lt.pattern.iter().any(|e| e.length > 0.0) && lt.pattern.iter().any(|e| e.length < 0.0));
    }

    #[test]
    fn 이름은_제_색이고_면적은_여린_먹이다() {
        // 면을 전부 회색으로 하면 공간이 여럿일 때 구분이 안 된다 —
        // 그래서 색은 **글자**에 남는다
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 8.0)]);
        let t = texts(&d);
        assert_eq!(t[0].2, Color::True(Rgb(0x1a, 0x73, 0xe8)), "이름 색이 다르다");
        assert_eq!(t[1].2, Color::True(AREA_INK), "면적 색이 다르다");
        // 면과 테두리는 공간마다 같은 회색이다
        assert_eq!(d.model.iter().next().expect("면").common.color, Color::True(WASH));
    }

    #[test]
    fn 이름이_위_면적이_아래다() {
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 8.0)]);
        let t = texts(&d);
        assert!(t[0].3.y > t[1].3.y, "이름 {:?} 면적 {:?}", t[0].3, t[1].3);
        // 둘 다 가운데 x 에 선다 (0..100 의 꼭짓점 평균 = 50)
        assert_eq!(t[0].3.x, 50.0);
        assert_eq!(t[1].3.x, 50.0);
    }

    #[test]
    fn 글자_크기는_화면과_같다() {
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 8.0)]);
        let t = texts(&d);
        assert_eq!(t[0].1, 14.0);
        assert_eq!(t[1].1, 11.0);
    }

    #[test]
    fn 이름이_비면_미지정_이다() {
        for name in ["", "   "] {
            let mut d = Drawing::new_metric();
            add_zones(&mut d, &[square(name, 8.0)]);
            assert_eq!(texts(&d)[0].0, "미지정", "{name:?}");
        }
    }

    #[test]
    fn 면적은_소수_첫째까지다() {
        for (area, want) in [(8.25, "8.3m²"), (8.0, "8.0m²"), (12.34, "12.3m²")] {
            let mut d = Drawing::new_metric();
            add_zones(&mut d, &[square("거실", area)]);
            assert_eq!(texts(&d)[1].0, want, "{area}");
        }
    }


    #[test]
    fn 면적을_다시_재지_않는다() {
        // 폴리곤에서 다시 재면 화면이 띄우는 숫자와 달라지고, 그러면 둘 중
        // 어느 게 맞는지 아무도 못 가린다. 들어온 값을 그대로 쓴다.
        let mut z = square("거실", 123.4);
        z.points = vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)];
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[z]);
        assert_eq!(texts(&d)[1].0, "123.4m²");
    }

    #[test]
    fn 꼭짓점_평균을_쓴다() {
        // 무게중심(면적 가중)이 아니다. 화면이 꼭짓점 평균이라 그걸 따라간다 —
        // 오목한 공간에서 둘이 눈에 띄게 벌어진다.
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(90.0, 0.0),
            Vec2::new(90.0, 10.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 90.0),
            Vec2::new(0.0, 90.0),
        ];
        let c = label_anchor(&pts);
        assert!((c.x - 33.333_333_333_333_33).abs() < 1e-9, "{c:?}");
        assert!((c.y - 33.333_333_333_333_33).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn 점이_셋_미만이면_안_넣는다() {
        let mut two = square("거실", 8.0);
        two.points.truncate(2);
        let mut nan = square("거실", 8.0);
        nan.points[1] = Vec2::new(f64::NAN, 0.0);

        let mut d = Drawing::new_metric();
        assert_eq!(add_zones(&mut d, &[two, nan]), 0);
        assert!(d.model.is_empty());
        // 하나도 안 넣었으면 선종도 만들지 않는다
        assert!(d.linetype(DASH).is_none());
    }

    #[test]
    fn 레이어가_엔티티마다_붙는다() {
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 8.0)]);
        assert!(d.model.iter().all(|e| e.common.layer == "A-ZONE"), "레이어가 빠졌다");
    }

    #[test]
    fn 여러_개를_넣는다() {
        let mut d = Drawing::new_metric();
        let mut b = square("주방", 5.0);
        b.points = b.points.iter().map(|p| *p + Vec2::new(200.0, 0.0)).collect();
        assert_eq!(add_zones(&mut d, &[square("거실", 8.0), b]), 2);
        assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Hatch(_))).count(), 2);
        assert_eq!(texts(&d).len(), 4);
    }

    #[test]
    fn 공간_하나의_조각들이_한_덩어리가_된다() {
        // 면·테두리·이름·면적 글자가 같은 id 를 들어야 "공간" 이라는 것이
        // 도면 안에 있는 것이다.
        let mut d = Drawing::new_metric();
        let mut z = square("거실", 8.0);
        z.id = "shape:zone1".into();
        add_zones(&mut d, &[z]);

        assert!(d.model.len() >= 3, "엔티티가 {}개뿐이다", d.model.len());
        for (i, e) in d.model.iter().enumerate() {
            let g = crate::identity::of(e).unwrap_or_else(|| panic!("{i}번째에 정체성이 없다"));
            assert_eq!(g.kind, crate::identity::ZONE);
            assert_eq!(g.ids, vec!["shape:zone1"]);
        }
    }

    #[test]
    fn 두_공간이_서로_안_물린다() {
        let mut d = Drawing::new_metric();
        let mut a = square("거실", 8.0);
        a.id = "shape:a".into();
        let mut b = square("주방", 5.0);
        b.id = "shape:b".into();
        b.points = b.points.iter().map(|p| *p + Vec2::new(200.0, 0.0)).collect();
        add_zones(&mut d, &[a, b]);

        let ids: Vec<_> = d
            .model
            .iter()
            .map(|e| crate::identity::of(e).expect("정체성이 없다").ids)
            .collect();
        let n_a = ids.iter().filter(|x| *x == &vec!["shape:a".to_string()]).count();
        let n_b = ids.iter().filter(|x| *x == &vec!["shape:b".to_string()]).count();
        assert_eq!(n_a, n_b, "조각 수가 다르다 — 범위가 어긋났다: {ids:?}");
        assert_eq!(n_a + n_b, ids.len());
    }

    #[test]
    fn 출처_없는_공간에는_안_붙인다() {
        let mut d = Drawing::new_metric();
        add_zones(&mut d, &[square("거실", 8.0)]);
        assert!(d.model.iter().all(|e| crate::identity::of(e).is_none()));
    }
}
