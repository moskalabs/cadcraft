//! Bimove 문·창을 엔진 엔티티로 옮긴다.
//!
//! ── 갭을 어떻게 뚫나 ──
//!
//! 문과 창은 벽 **위에** 놓인다. 지금 화면(`src/shapes/DoorShape.tsx`)은 벽
//! 위에 흰 사각형을 덮어 벽을 지우고, 그 위에 잼브 두 줄을 긋는다. 벽을 실제로
//! 자르지는 않는다.
//!
//! 엔진에도 그 "덮어서 지우는" 것이 있다 — **WIPEOUT** 이다. 그리는 차례상
//! 앞에 그려진 것을 가리고, 가릴 때 **배경색**으로 칠한다. 흰 사각형과 다른
//! 점이 그거다: 다크 모드에서 흰 네모가 뜨지 않는다. 배경색을 여기까지
//! 내려보내 추측할 필요가 없다는 뜻이다 ([[wall.rs]] 의 속 채움은 아직 그걸
//! 추측하고 있고, 그래서 거기엔 주석이 붙어 있다).
//!
//! 그리는 차례에 기대므로 **문·창은 벽보다 나중에** 넣어야 한다. 엔진의
//! 렌더러는 엔티티 차례대로 칠한다 (여러 코어로 나눠 만들어도 "joined in
//! drawing order" 다). `drawing_from_bimove` 가 벽을 넣은 **뒤에** 이걸 부른다.
//!
//! WIPEOUT 은 기본으로 테두리를 네 변 다 그린다 (`WIPEOUTFRAME`). 그걸 켜 두면
//! 벽 면선이 갭을 가로질러 다시 생겨서 화면과 달라진다. 그래서 `WIPEOUTFRAME`
//! 을 0 으로 두고 잼브만 직접 긋는다.
//!
//! ── 왜 벽을 자르지 않나 ──
//!
//! 자르는 게 구조적으로는 맞다. 다만 지금은 벽이 MLINE 한 줄기로 **묶여**
//! 있어서, 자르려면 줄기를 쪼개고 쪼갠 자리의 캡을 다시 판단해야 한다. 그러면
//! `wall.rs` 의 묶기와 문·창이 서로 얽힌다. tldraw 와 나란히 돌리는 동안은
//! **지금 화면과 같은 그림**이 나오는 쪽이 낫다. 자르는 건 tldraw 를 걷어낸
//! 뒤에 할 일이다.
//!
//! ── 좌표 ──
//!
//! Bimove 의 문·창은 `x,y` 가 **가운데**다 (벽선에 붙은 그 점). 벽과 다르다 —
//! 벽은 `x,y` 가 시작점이고 `props.x2,y2` 가 거기서의 오프셋이다. 제 좌표계는
//! x 가 벽을 따라가고 y 가 벽을 가로지른다.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Arc, Drawing, Entity, EntityKind, Line, Wipeout};
use cadcraft_geom::{Vec2, Vec3};
use std::f64::consts::{PI, TAU};

/// 문짝이 열리는 쪽과 경첩 쪽.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swing {
    /// `+1` 이면 +y 쪽, `-1` 이면 -y 쪽 (제 좌표계에서).
    pub side: f64,
    /// 경첩을 좌우로 뒤집는다.
    pub flipped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpeningKind {
    Door(Swing),
    Window,
}

/// 문 또는 창 하나. 좌표는 **캔버스** 것이다 — y 뒤집기는 이 안에서 한다.
///
/// 벽(`WallSeg`)과 달리 캔버스 좌표를 받는다. 돌리기와 뒤집기가 한 자리에
/// 모여 있어야 거울 변환을 한 번만 따지면 되기 때문이다 (아래 `frame` 참고).
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    /// 이 문·창을 만든 Bimove 도형의 id. 엔티티 다섯이 이걸로 **한 덩어리**가
    /// 된다 (`crate::identity`). 빈 문자열이면 심지 않는다.
    pub id: String,
    /// 캔버스에서의 가운데.
    pub center: Vec2,
    /// 캔버스에서의 회전 (라디안). 벽 방향이다.
    pub rotation: f64,
    pub width: f64,
    pub thickness: f64,
    pub layer: String,
    pub kind: OpeningKind,
}

/// 잼브·문짝·호의 색.
///
/// 화면은 `#222`·`#444`·`#666` 의 회색 단계를 쓴다. 그걸 그대로 박으면
/// **어두운 배경에서 전부 묻는다** — `ByLayer` 로 두면 엔진이 배경을 보고
/// 잉크를 고른다 ([[wall.rs]] 의 윤곽선도 같은 이유로 그렇게 한다).
///
/// 회색 단계가 주던 구분은 어차피 화면 렌더에서 거의 사라진다: 선 굵기는 mm
/// 단위라 `px_per_mm` 이 0 인 화면에서는 모두 hairline 로 그려진다. 호는 파선이
/// 구분해 주므로 색까지 쓸 필요가 없다.
const INK: Color = Color::ByLayer;

/// 유리만 제 색을 쓴다. 파란색은 밝은 배경에서도 어두운 배경에서도 읽힌다.
const GLASS: Color = Color::True(Rgb(0x55, 0xaa, 0xff));

/// 스윙 호에 쓰는 파선. 화면의 `strokeDasharray="3,2"` 와 같은 길이다.
pub const DASH: &str = "BIMOVE_DASH";

/// 제 좌표 → 엔진 좌표. **거울 변환이다.**
///
/// 캔버스는 y 가 아래로, 엔진은 위로 간다. 돌리기는 캔버스에서 하고 y 를
/// 마지막에 뒤집으므로, 합쳐 보면 뒤집기가 섞인 변환이 된다 (행렬식 -1).
/// 그래서 **돌아가는 방향이 뒤바뀐다** — 제 좌표에서 반시계로 쓸던 호가
/// 엔진에서는 시계 방향이 된다. 호를 넣을 때 그걸 되짚어야 한다
/// (`door` 의 주석 참고).
///
/// 각도를 따로 보정하지는 않는다. 축 둘을 돌려서 그대로 들고 오기 때문이다.
fn frame(o: &Opening) -> (Vec2, Vec2, Vec2) {
    let (c, s) = (o.rotation.cos(), o.rotation.sin());
    let origin = Vec2::new(o.center.x, -o.center.y);
    // x 축은 벽을 따라, y 축은 벽을 가로질러 — 둘 다 y 를 뒤집은 뒤의 것
    (origin, Vec2::new(c, -s), Vec2::new(-s, -c))
}

/// 쓸 수 있는 문·창인가. 폭이나 두께가 0 이면 그릴 것이 없다.
pub fn usable(o: &Opening) -> bool {
    o.center.is_finite()
        && o.rotation.is_finite()
        && o.width > 0.0
        && o.thickness > 0.0
        && o.width.is_finite()
        && o.thickness.is_finite()
}

/// 파선 선종을 도면에 넣어 둔다. 없으면 호가 실선으로 나온다.
pub fn register_linetypes(drawing: &mut Drawing) {
    if drawing.linetype(DASH).is_none() {
        drawing
            .linetypes
            .push(cadcraft_doc::Linetype::simple(DASH, "문 스윙 호", &[3.0, -2.0]));
    }
}

/// 한 엔티티를 레이어·색·선종까지 얹어 넣는다.
fn push(drawing: &mut Drawing, layer: &str, color: Color, lt: Option<&str>, kind: EntityKind) {
    let h = drawing.new_handle();
    let mut e = Entity::new(h, kind);
    e.common.layer = layer.to_string();
    e.common.color = color;
    if let Some(name) = lt {
        e.common.linetype = name.to_string();
    }
    drawing.model.push(e);
}

fn seg(drawing: &mut Drawing, layer: &str, color: Color, lt: Option<&str>, a: Vec2, b: Vec2) {
    push(
        drawing,
        layer,
        color,
        lt,
        EntityKind::Line(Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) }),
    );
}

/// 벽을 가리는 사각형과 잼브 두 줄. 문과 창이 똑같이 쓴다.
fn gap_and_jambs(drawing: &mut Drawing, o: &Opening) {
    let (c, u, v) = frame(o);
    let (hw, ht) = (o.width / 2.0, o.thickness / 2.0);
    let at = |lx: f64, ly: f64| c + u * lx + v * ly;

    // 벽을 가린다 — 배경색으로 칠하는 마스크다
    push(
        drawing,
        &o.layer,
        INK,
        None,
        EntityKind::Wipeout(Wipeout {
            boundary: vec![at(-hw, -ht), at(hw, -ht), at(hw, ht), at(-hw, ht)],
        }),
    );

    // 잼브 — 벽을 가로지르는 두 줄. 벽 면선은 위에서 지워졌으므로 이게 갭을 닫는다
    seg(drawing, &o.layer, INK, None, at(-hw, -ht), at(-hw, ht));
    seg(drawing, &o.layer, INK, None, at(hw, -ht), at(hw, ht));
}

/// 각도를 `[0, TAU)` 로.
fn wrap(a: f64) -> f64 {
    let m = a % TAU;
    if m < 0.0 { m + TAU } else { m }
}

fn door(drawing: &mut Drawing, o: &Opening, sw: Swing) {
    gap_and_jambs(drawing, o);

    let (c, u, v) = frame(o);
    let hw = o.width / 2.0;
    let at = |lx: f64, ly: f64| c + u * lx + v * ly;

    let f = if sw.flipped { -1.0 } else { 1.0 };
    let hinge = at(-hw * f, 0.0);
    let free = at(hw * f, 0.0);
    // 문짝은 경첩에서 열린 쪽으로 제 폭만큼 뻗는다
    let tip = at(-hw * f, sw.side * o.width);

    seg(drawing, &o.layer, INK, None, hinge, tip);

    // 스윙 호 — 경첩을 중심으로 `tip` 에서 `free` 까지의 **90°** 짜리다
    // (화면의 `A` 명령도 large-arc-flag 가 0 이다). 90° 라는 걸 알고 있으니
    // 화면의 sweep-flag 를 되짚을 필요가 없다: 엔진 좌표에서 두 끝의 각을
    // 재고, 반시계로 90° 가 되는 쪽을 시작으로 잡으면 된다. 거울 변환 때문에
    // 어느 쪽이 시작인지가 `flipped`·`swing` 조합마다 뒤집히는데, 이렇게
    // 재면 그 네 경우를 따로 쓰지 않아도 맞는다.
    let (ta, fa) = (wrap((tip - hinge).angle()), wrap((free - hinge).angle()));
    let (start, end) = if wrap(fa - ta) <= PI { (ta, fa) } else { (fa, ta) };
    push(
        drawing,
        &o.layer,
        INK,
        Some(DASH),
        EntityKind::Arc(Arc {
            center: Vec3::new(hinge.x, hinge.y, 0.0),
            radius: o.width,
            start,
            end,
        }),
    );
}

fn window(drawing: &mut Drawing, o: &Opening) {
    gap_and_jambs(drawing, o);

    let (c, u, v) = frame(o);
    let (hw, ht) = (o.width / 2.0, o.thickness / 2.0);
    let at = |lx: f64, ly: f64| c + u * lx + v * ly;
    // 화면의 `gap = ht * 0.4`
    let g = ht * 0.4;

    // 유리 — 벽을 따라가는 평행선 둘
    seg(drawing, &o.layer, GLASS, None, at(-hw, -g), at(hw, -g));
    seg(drawing, &o.layer, GLASS, None, at(-hw, g), at(hw, g));
}

/// 문·창을 도면에 넣는다. **벽을 넣은 뒤에** 불러야 한다 (모듈 주석 참고).
///
/// @returns 넣은 개수. 못 쓰는 것은 세지 않는다 — 부르는 쪽이 그 차이를 알려야 한다.
pub fn add_openings(drawing: &mut Drawing, openings: &[Opening]) -> usize {
    if openings.iter().any(|o| matches!(o.kind, OpeningKind::Door(_))) {
        register_linetypes(drawing);
    }
    // 테두리를 네 변 다 그리면 벽 면선이 갭을 가로질러 되살아난다
    if openings.iter().any(usable) {
        drawing.header.set_i64("WIPEOUTFRAME", 0);
    }

    let mut n = 0;
    for o in openings.iter().filter(|o| usable(o)) {
        // 하나가 엔티티 다섯이 된다. 넣기 전의 길이를 재 두었다가 넣은 뒤에
        // 그 범위를 한 번에 찍는다 — 그래야 다섯이 같은 문이 된다.
        let from = drawing.model.len();
        match o.kind {
            OpeningKind::Door(sw) => door(drawing, o, sw),
            OpeningKind::Window => window(drawing, o),
        }
        let kind = match o.kind {
            OpeningKind::Door(_) => crate::identity::DOOR,
            OpeningKind::Window => crate::identity::WINDOW,
        };
        crate::identity::stamp_one(drawing, from, kind, &o.id);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn door_at(x: f64, y: f64, rot: f64, side: f64, flipped: bool) -> Opening {
        Opening {
            id: String::new(),
            center: Vec2::new(x, y),
            rotation: rot,
            width: 80.0,
            thickness: 20.0,
            layer: "A-DOOR".into(),
            kind: OpeningKind::Door(Swing { side, flipped }),
        }
    }

    fn win_at(x: f64, y: f64, rot: f64) -> Opening {
        Opening {
            id: String::new(),
            center: Vec2::new(x, y),
            rotation: rot,
            width: 60.0,
            thickness: 20.0,
            layer: "A-WIN".into(),
            kind: OpeningKind::Window,
        }
    }

    /// 첫 엔티티의 가림 경계. 갭이 **먼저** 와야 하므로 첫 번째가 맞다.
    fn first_wipeout(d: &Drawing) -> Vec<Vec2> {
        match &d.model.iter().next().expect("엔티티가 없다").kind {
            EntityKind::Wipeout(w) => w.boundary.clone(),
            other => panic!("첫 엔티티가 wipeout 이 아니다: {other:?}"),
        }
    }

    fn kinds(d: &Drawing) -> Vec<&'static str> {
        d.model
            .iter()
            .map(|e| match e.kind {
                EntityKind::Wipeout(_) => "wipeout",
                EntityKind::Line(_) => "line",
                EntityKind::Arc(_) => "arc",
                _ => "기타",
            })
            .collect()
    }

    fn arcs(d: &Drawing) -> Vec<Arc> {
        d.model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Arc(a) => Some(a.clone()),
                _ => None,
            })
            .collect()
    }

    fn lines(d: &Drawing) -> Vec<(Vec2, Vec2)> {
        d.model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Line(l) => Some((l.a.xy(), l.b.xy())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn 문은_갭_잼브_문짝_호로_나온다() {
        let mut d = Drawing::new_metric();
        assert_eq!(add_openings(&mut d, &[door_at(0.0, 0.0, 0.0, 1.0, false)]), 1);
        // 갭이 **먼저** 와야 한다 — 나중에 오면 잼브와 문짝을 지운다
        assert_eq!(kinds(&d), vec!["wipeout", "line", "line", "line", "arc"]);
    }

    #[test]
    fn 창은_갭_잼브_유리로_나온다() {
        let mut d = Drawing::new_metric();
        assert_eq!(add_openings(&mut d, &[win_at(0.0, 0.0, 0.0)]), 1);
        assert_eq!(kinds(&d), vec!["wipeout", "line", "line", "line", "line"]);
        assert!(arcs(&d).is_empty(), "창에는 스윙 호가 없다");
    }

    #[test]
    fn 갭은_폭과_두께만큼이고_y_가_뒤집힌다() {
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[win_at(100.0, 50.0, 0.0)]);
        let b = first_wipeout(&d);
        let xs: Vec<f64> = b.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = b.iter().map(|p| p.y).collect();
        // 폭 60, 두께 20, 가운데 (100, 50) → 엔진에서는 y 가 -50
        assert_eq!(xs.iter().cloned().fold(f64::MAX, f64::min), 70.0);
        assert_eq!(xs.iter().cloned().fold(f64::MIN, f64::max), 130.0);
        assert_eq!(ys.iter().cloned().fold(f64::MAX, f64::min), -60.0);
        assert_eq!(ys.iter().cloned().fold(f64::MIN, f64::max), -40.0);
    }

    #[test]
    fn 돌린_창의_갭도_같이_돌아간다() {
        // 90° 돌리면 폭과 두께가 자리를 바꾼다
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[win_at(0.0, 0.0, PI / 2.0)]);
        let b = first_wipeout(&d);
        let w = b.iter().map(|p| p.x).fold(f64::MIN, f64::max)
            - b.iter().map(|p| p.x).fold(f64::MAX, f64::min);
        let h = b.iter().map(|p| p.y).fold(f64::MIN, f64::max)
            - b.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        assert!((w - 20.0).abs() < 1e-9, "가로 {w}");
        assert!((h - 60.0).abs() < 1e-9, "세로 {h}");
    }

    #[test]
    fn 스윙_호는_90도짜리고_경첩이_중심이다() {
        // 네 조합 모두 — 거울 변환 때문에 시작·끝이 뒤집히는 자리가 여기다
        for &side in &[1.0, -1.0] {
            for &flipped in &[false, true] {
                let mut d = Drawing::new_metric();
                add_openings(&mut d, &[door_at(0.0, 0.0, 0.0, side, flipped)]);
                let a = &arcs(&d)[0];
                let span = wrap(a.end - a.start);
                assert!(
                    (span - PI / 2.0).abs() < 1e-9,
                    "side {side} flipped {flipped} — 호가 {}° 다",
                    span.to_degrees()
                );
                assert_eq!(a.radius, 80.0, "반지름은 문 폭이다");
                // 경첩은 ±hw 중 하나에 있다
                assert!((a.center.x.abs() - 40.0).abs() < 1e-9, "중심 {:?}", a.center);
                assert!(a.center.y.abs() < 1e-9, "중심 {:?}", a.center);
            }
        }
    }

    #[test]
    fn 스윙_호가_문짝_끝과_반대쪽_잼브를_지난다() {
        // 호가 엉뚱한 반원으로 가면 문이 벽 반대편으로 열린 것처럼 보인다.
        // 양 끝이 실제로 그 두 점인지 좌표로 본다.
        let o = door_at(0.0, 0.0, 0.0, 1.0, false);
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[o.clone()]);
        let a = &arcs(&d)[0];
        let c = a.center.xy();
        let p = |ang: f64| c + Vec2::new(ang.cos(), ang.sin()) * a.radius;
        let (s, e) = (p(a.start), p(a.end));

        // 문짝 끝 = 문짝 선의 자유단, 반대쪽 잼브 = 경첩에서 폭만큼 간 자리
        let leaf = lines(&d).into_iter().find(|(x, _)| (*x - c).len() < 1e-9).expect("문짝");
        let tip = leaf.1;
        let other = Vec2::new(c.x + o.width * (if c.x < 0.0 { 1.0 } else { -1.0 }), c.y);

        let hits = |q: Vec2| (q - s).len() < 1e-6 || (q - e).len() < 1e-6;
        assert!(hits(tip), "문짝 끝 {tip:?} 이 호의 끝이 아니다 ({s:?}, {e:?})");
        assert!(hits(other), "반대쪽 {other:?} 이 호의 끝이 아니다 ({s:?}, {e:?})");
    }

    #[test]
    fn 문짝은_열리는_쪽으로_뻗는다() {
        // side 를 뒤집으면 문짝이 벽 반대편으로 가야 한다
        let mut up = Drawing::new_metric();
        add_openings(&mut up, &[door_at(0.0, 0.0, 0.0, 1.0, false)]);
        let mut down = Drawing::new_metric();
        add_openings(&mut down, &[door_at(0.0, 0.0, 0.0, -1.0, false)]);

        let tip = |d: &Drawing| {
            let c = arcs(d)[0].center.xy();
            lines(d).into_iter().find(|(x, _)| (*x - c).len() < 1e-9).expect("문짝").1.y
        };
        assert!(tip(&up) * tip(&down) < 0.0, "{} / {}", tip(&up), tip(&down));
        assert!((tip(&up).abs() - 80.0).abs() < 1e-9, "문짝 길이는 폭이다");
    }

    #[test]
    fn 경첩을_뒤집으면_좌우가_바뀐다() {
        let mut a = Drawing::new_metric();
        add_openings(&mut a, &[door_at(0.0, 0.0, 0.0, 1.0, false)]);
        let mut b = Drawing::new_metric();
        add_openings(&mut b, &[door_at(0.0, 0.0, 0.0, 1.0, true)]);
        assert!(
            arcs(&a)[0].center.x * arcs(&b)[0].center.x < 0.0,
            "{} / {}",
            arcs(&a)[0].center.x,
            arcs(&b)[0].center.x
        );
    }

    #[test]
    fn 유리는_벽을_따라가고_두께_안에_있다() {
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[win_at(0.0, 0.0, 0.0)]);
        // 잼브 둘(가로지름) 과 유리 둘(따라감) — 따라가는 것만 고른다
        let glass: Vec<_> = lines(&d).into_iter().filter(|(a, b)| (a.x - b.x).abs() > 1e-9).collect();
        assert_eq!(glass.len(), 2, "유리가 둘이 아니다");
        for (a, b) in glass {
            assert!((a.y - b.y).abs() < 1e-9, "유리가 기울었다");
            // gap = ht * 0.4 = 4
            assert!((a.y.abs() - 4.0).abs() < 1e-9, "유리 간격 {}", a.y);
            assert!((a.x + 30.0).abs() < 1e-9 && (b.x - 30.0).abs() < 1e-9, "{a:?} {b:?}");
        }
    }

    #[test]
    fn 파선_선종을_도면에_넣는다() {
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[door_at(0.0, 0.0, 0.0, 1.0, false)]);
        let lt = d.linetype(DASH).expect("파선이 없다 — 호가 실선으로 나온다");
        assert!(!lt.pattern.is_empty());
        // 켬·끔이 섞여 있어야 파선이다
        assert!(lt.pattern.iter().any(|e| e.length > 0.0) && lt.pattern.iter().any(|e| e.length < 0.0));
        assert_eq!(arcs(&d)[0].radius, 80.0);
        let arc_lt = d
            .model
            .iter()
            .find(|e| matches!(e.kind, EntityKind::Arc(_)))
            .map(|e| e.common.linetype.clone())
            .expect("호가 없다");
        assert_eq!(arc_lt, DASH, "호에 파선이 안 붙었다");
    }

    #[test]
    fn 창만_있으면_파선을_안_만든다() {
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[win_at(0.0, 0.0, 0.0)]);
        assert!(d.linetype(DASH).is_none(), "쓰지도 않는 선종을 넣었다");
    }

    #[test]
    fn 테두리를_끈다() {
        // 켜 두면 벽 면선이 갭을 가로질러 되살아난다
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[win_at(0.0, 0.0, 0.0)]);
        assert_eq!(d.header.i64("WIPEOUTFRAME", 1), 0);
    }

    #[test]
    fn 못_쓰는_것은_안_넣는다() {
        let mut zero = door_at(0.0, 0.0, 0.0, 1.0, false);
        zero.width = 0.0;
        let mut thin = win_at(0.0, 0.0, 0.0);
        thin.thickness = 0.0;
        let mut nan = win_at(f64::NAN, 0.0, 0.0);
        nan.width = 10.0;

        let mut d = Drawing::new_metric();
        assert_eq!(add_openings(&mut d, &[zero, thin, nan]), 0);
        assert!(d.model.is_empty());
        // 하나도 안 넣었으면 헤더도 건드리지 않는다
        assert_eq!(d.header.i64("WIPEOUTFRAME", 1), 1);
    }

    #[test]
    fn 잉크는_배경에_맡기고_유리만_제_색이다() {
        // 회색을 박으면 어두운 배경에서 묻는다 — 그래서 `ByLayer` 다.
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[door_at(0.0, 0.0, 0.0, 1.0, false)]);
        assert!(
            d.model.iter().all(|e| e.common.color == Color::ByLayer),
            "문에 색이 박혀 있다"
        );

        let mut w = Drawing::new_metric();
        add_openings(&mut w, &[win_at(0.0, 0.0, 0.0)]);
        let glass: Vec<Color> = w
            .model
            .iter()
            .filter(|e| e.common.color != Color::ByLayer)
            .map(|e| e.common.color)
            .collect();
        assert_eq!(glass, vec![GLASS, GLASS], "유리만 제 색이어야 한다");
    }

    #[test]
    fn 레이어가_엔티티마다_붙는다() {
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[door_at(0.0, 0.0, 0.0, 1.0, false)]);
        assert!(d.model.iter().all(|e| e.common.layer == "A-DOOR"), "레이어가 빠졌다");
    }

    #[test]
    fn 여러_개를_차례대로_넣는다() {
        let mut d = Drawing::new_metric();
        let n = add_openings(
            &mut d,
            &[door_at(0.0, 0.0, 0.0, 1.0, false), win_at(200.0, 0.0, 0.0), door_at(400.0, 0.0, PI, -1.0, true)],
        );
        assert_eq!(n, 3);
        assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Wipeout(_))).count(), 3);
        assert_eq!(arcs(&d).len(), 2);
    }

    #[test]
    fn 문_하나의_엔티티들이_한_덩어리가_된다() {
        // 여기가 요점이다. 다섯 엔티티가 같은 id 를 들어야 "문짝 선 하나만
        // 지워지는" 일이 없어진다.
        let mut d = Drawing::new_metric();
        let mut door = door_at(0.0, 0.0, 0.0, 1.0, false);
        door.id = "shape:door1".into();
        add_openings(&mut d, &[door]);

        let got: Vec<_> = d.model.iter().map(|e| crate::identity::of(e)).collect();
        assert!(got.len() >= 5, "엔티티가 {}개뿐이다", got.len());
        for (i, g) in got.iter().enumerate() {
            let g = g.as_ref().unwrap_or_else(|| panic!("{i}번째 엔티티에 정체성이 없다"));
            assert_eq!(g.kind, crate::identity::DOOR);
            assert_eq!(g.ids, vec!["shape:door1"]);
        }
    }

    #[test]
    fn 문과_창이_섞여도_서로_안_물린다() {
        // 범위를 잘못 재면 창의 가림막이 앞 문의 것으로 찍힌다 — 창을
        // 고르면 문이 같이 지워진다.
        let mut d = Drawing::new_metric();
        let mut door = door_at(0.0, 0.0, 0.0, 1.0, false);
        door.id = "shape:d".into();
        let mut win = win_at(200.0, 0.0, 0.0);
        win.id = "shape:w".into();
        add_openings(&mut d, &[door, win]);

        let mut doors = 0;
        let mut wins = 0;
        for e in d.model.iter() {
            match crate::identity::of(e).expect("정체성이 없다") {
                g if g.ids == vec!["shape:d".to_string()] => {
                    assert_eq!(g.kind, crate::identity::DOOR);
                    doors += 1;
                }
                g => {
                    assert_eq!(g.ids, vec!["shape:w".to_string()]);
                    assert_eq!(g.kind, crate::identity::WINDOW);
                    wins += 1;
                }
            }
        }
        assert_eq!(doors, 5, "문은 가림 + 잼브 둘 + 문짝 + 호 다섯이다");
        assert_eq!(wins, 5, "창은 가림 + 잼브 둘 + 유리 둘 다섯이다");
    }

    #[test]
    fn 출처_없는_문에는_안_붙인다() {
        let mut d = Drawing::new_metric();
        add_openings(&mut d, &[door_at(0.0, 0.0, 0.0, 1.0, false)]);
        assert!(d.model.iter().all(|e| crate::identity::of(e).is_none()));
    }
}
