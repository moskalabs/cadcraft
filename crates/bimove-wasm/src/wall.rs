//! Bimove 벽체를 엔진 MLINE 으로 옮긴다.
//!
//! ── 왜 MLINE 인가 ──
//!
//! Bimove 의 벽은 도형 하나가 **선분 하나**다. 두께를 가진 선분이고, 꺾인 자리는
//! 이웃을 끝점으로 찾아 miter 를 손으로 계산한다 (`src/shapes/WallShape.tsx` 의
//! `miterOffset`). 그 계산은 세 번 틀렸다 — 바깥만 늘리고 안쪽을 안 당겼고,
//! 한도를 제 두께로만 잡아 두께가 다르면 두 벽이 다른 자리에 멈췄고, 제 miter
//! 보다 짧은 벽에서 도형이 뒤집혔다.
//!
//! 엔진에는 그게 이미 있다. MLINE 은 **중심선 + 평행 요소**이고
//! `MLine::miters()` 가 꼭짓점마다 이등분 벡터를 길이 보정까지 해서 내놓는다.
//! 꺾인 코너가 계산이 아니라 **구조**로 맞물린다는 뜻이다. 캡과 속 채움도
//! 딸려 있다. DXF 로 내보낼 때도 MLINE 이 평면도의 벽 그 자체다.
//!
//! 그래서 벽을 하나씩 옮기지 않고 **이어진 벽을 한 MLINE 으로 묶는다.** 이
//! 파일이 하는 일은 그 묶기다.
//!
//! ── 묶지 못하는 자리 ──
//!
//! MLINE 의 꼭짓점은 이웃이 **둘**이다. 벽 셋이 한 점에서 만나면(ㅏ·ㅗ 자)
//! 거기서 끊어야 한다. 끊은 자리는 **캡을 끈다** — 캡을 그리면 코너를
//! 가로지르는 선이 되고, 그게 한이 "꺾이는 지점 안겹치게" 라고 한 바로 그
//! 증상이다. 끝이 **정말 비어 있을 때만** 막는다.
//!
//! 두께나 레이어가 다른 벽끼리도 못 묶는다. MLINE 하나는 스타일 하나다.

use cadcraft_color::Color;
use cadcraft_doc::{
    Drawing, Entity, EntityKind, MLine, MLineCaps, MLineElement, MLineJust, MLineStyle, MLineVertex,
};
use cadcraft_geom::{Vec2, Vec3};
use std::collections::HashMap;

/// 벽 하나 (중심선 선분). 좌표는 **엔진** 것이다 — 페이지 변환과 y 뒤집기가 끝난 값.
#[derive(Clone, Debug, PartialEq)]
pub struct WallSeg {
    pub a: Vec2,
    pub b: Vec2,
    pub thickness: f64,
    pub layer: String,
    /// 윤곽선 색.
    pub color: Color,
    /// 속 색. `None` 이면 안 칠한다.
    pub fill: Option<Color>,
}

/// 끝점이 만났다고 볼 거리.
///
/// `WallShape.tsx` 의 `snapR = Math.max(JOIN_SNAP_R, half * 0.5)` 과 같은 값을
/// 쓴다. 두 코드가 **같은 벽을 이웃으로 보아야** 화면과 엔진이 같은 그림을
/// 그린다 — tldraw 를 걷어내는 동안은 둘이 나란히 돌기 때문이다.
const JOIN_SNAP_R: f64 = 8.0;

fn join_r(thickness: f64) -> f64 {
    JOIN_SNAP_R.max(thickness * 0.5)
}

/// 한 MLINE 이 될 벽 줄기.
#[derive(Clone, Debug, PartialEq)]
pub struct WallChain {
    /// 중심선 꼭짓점. `closed` 면 마지막 점을 되풀이하지 않는다.
    pub pts: Vec<Vec2>,
    pub closed: bool,
    /// 시작 끝이 **정말 비어 있는가**. 아니면 캡을 끈다.
    pub free_start: bool,
    pub free_end: bool,
    pub thickness: f64,
    pub layer: String,
    pub color: Color,
    pub fill: Option<Color>,
}

/// 묶을 수 있는 벽끼리 가르는 열쇠. MLINE 하나는 스타일 하나라서 필요하다.
///
/// 두께는 **정수로 줄여서** 비교한다. 같은 두께로 그린 벽이 저장·복원을
/// 거치며 1e-13 쯤 어긋나는 일이 있고, 그걸 다른 두께로 보면 줄기가 벽마다
/// 끊어져 코너가 전부 캡으로 막힌다.
type GroupKey = (String, i64, Color, Option<Color>);

fn group_key(w: &WallSeg) -> GroupKey {
    (w.layer.clone(), (w.thickness * 1000.0).round() as i64, w.color, w.fill)
}

/// 두 점이 같은 자리인가.
fn same(p: Vec2, q: Vec2, r: f64) -> bool {
    p.dist(q) <= r
}

/// 쓸 수 있는 벽인가. 길이 0 과 NaN 을 걸러낸다 — 둘 다 miter 를 통째로 날린다.
fn usable(w: &WallSeg) -> bool {
    w.a.is_finite() && w.b.is_finite() && w.thickness > 0.0 && w.a.dist(w.b) > 1e-9
}

/// 벽들을 **이어진 줄기**로 묶는다.
///
/// 들어온 차례를 지킨다 — 엔티티 순서가 실행마다 바뀌면 내보낸 DXF 와 렌더
/// 결과를 견줄 수 없다.
pub fn chain_walls(walls: &[WallSeg]) -> Vec<WallChain> {
    // 묶을 수 있는 것끼리 모은다 (첫 등장 순서 유지).
    let mut order: Vec<GroupKey> = Vec::new();
    let mut groups: HashMap<GroupKey, Vec<usize>> = HashMap::new();
    for (i, w) in walls.iter().enumerate() {
        if !usable(w) {
            continue;
        }
        let k = group_key(w);
        groups.entry(k.clone()).or_insert_with(|| {
            order.push(k.clone());
            Vec::new()
        });
        groups.get_mut(&k).expect("방금 넣었다").push(i);
    }

    let mut out = Vec::new();
    for k in &order {
        let idx = &groups[k];
        out.extend(chain_group(walls, idx));
    }
    out
}

/// 그 점에 끝점이 닿은 벽들. **제 것도 센다.**
///
/// 제 것을 빼지 않는 게 중요하다. 그래야 숫자 하나로 세 가지가 갈린다 —
/// `1` 은 비어 있는 끝, `2` 는 MLINE 의 꼭짓점이 될 코너, `3` 이상은 벽 셋이
/// 만난 자리다. 제 것을 빼면 "비어 있음" 과 "코너" 가 같은 0/1 로 뭉개진다.
fn ends_at(walls: &[WallSeg], idx: &[usize], p: Vec2, r: f64) -> Vec<usize> {
    idx.iter()
        .copied()
        .filter(|&j| usable(&walls[j]))
        .filter(|&j| same(walls[j].a, p, r) || same(walls[j].b, p, r))
        .collect()
}

/// 벽 **전부**에서 그 점에 닿은 끝 개수.
///
/// 캡을 켤지는 두께·레이어와 **무관하게** 봐야 한다. 두께가 다른 벽은 한
/// 줄기로 못 묶지만, 그래도 거기서 만나고 있으므로 캡을 그리면 코너를
/// 가로지르는 선이 된다. 묶기는 그룹 안에서, 캡은 전부에서 본다.
fn ends_everywhere(walls: &[WallSeg], p: Vec2, r: f64) -> usize {
    walls
        .iter()
        .filter(|w| usable(w))
        .filter(|w| same(w.a, p, r) || same(w.b, p, r))
        .count()
}

fn chain_group(walls: &[WallSeg], idx: &[usize]) -> Vec<WallChain> {
    let mut used = vec![false; idx.len()];
    // 전역 인덱스 → 이 그룹 안 자리.
    let slot: HashMap<usize, usize> = idx.iter().enumerate().map(|(s, &i)| (i, s)).collect();
    let mut out = Vec::new();

    for (start_slot, &start) in idx.iter().enumerate() {
        if used[start_slot] {
            continue;
        }
        let w0 = &walls[start];
        let r = join_r(w0.thickness);
        used[start_slot] = true;

        // 중심선을 양쪽으로 늘려 간다.
        let mut pts = vec![w0.a, w0.b];
        let mut closed = false;

        // ── 앞으로 (pts 의 끝에서) ──
        loop {
            let tip = *pts.last().expect("점이 둘 이상이다");
            // **고리 검사를 먼저** 한다. 첫 점으로 돌아왔으면 거기 닿은 벽은
            // 둘(처음과 마지막)이고 둘 다 이미 썼으므로, 밑의 검사만 두면
            // 아무 일도 안 하고 빠져나와 사각형이 열린 줄기로 남는다.
            if pts.len() > 2 && same(tip, pts[0], r) {
                pts.pop();
                closed = true;
                break;
            }
            // 둘이 만난 자리만 꼭짓점이 된다. 셋 이상이면 끊는다.
            let at = ends_at(walls, idx, tip, r);
            if at.len() != 2 {
                break;
            }
            let Some(&j) = at.iter().find(|&&j| !used[slot[&j]]) else { break };
            let nw = &walls[j];
            used[slot[&j]] = true;
            // 붙은 쪽 **반대** 끝을 잇는다.
            pts.push(if same(nw.a, tip, r) { nw.b } else { nw.a });
        }

        // ── 뒤로 (pts 의 앞에서) ──
        if !closed {
            loop {
                let tip = pts[0];
                let at = ends_at(walls, idx, tip, r);
                if at.len() != 2 {
                    break;
                }
                let Some(&j) = at.iter().find(|&&j| !used[slot[&j]]) else { break };
                let nw = &walls[j];
                used[slot[&j]] = true;
                pts.insert(0, if same(nw.a, tip, r) { nw.b } else { nw.a });
            }
        }

        // 끝이 **정말 비어 있는가**. 다른 벽이 붙어 있으면 캡을 끈다 — 캡은
        // 코너를 가로지르는 선이 된다.
        let free_start = closed || ends_everywhere(walls, pts[0], r) == 1;
        let free_end =
            closed || ends_everywhere(walls, *pts.last().expect("점이 있다"), r) == 1;

        out.push(WallChain {
            pts,
            closed,
            free_start,
            free_end,
            thickness: w0.thickness,
            layer: w0.layer.clone(),
            color: w0.color,
            fill: w0.fill,
        });
    }
    out
}

/// 줄기의 **열린 끝**을 상대 벽의 몸통까지 밀어낸다 (T 자 이음).
///
/// 벽 끝이 다른 벽의 **가운데**에 닿는 경우다. 끝점끼리 만난 게 아니라서
/// 줄기로 묶이지 않는다. 그대로 두면 줄기가 상대의 중심선에서 멈춰 상대 벽
/// 속에 캡이 하나 그려진다 — 벽 안에 선이 하나 지나가는 꼴이다.
///
/// 그래서 **상대의 먼 쪽 면까지** 밀어낸다. 미는 양은 `h / sin θ` 다
/// (`WallShape.tsx` 의 `tExt` 와 같다). 밀어낸 끝은 비어 있지 않으므로 캡도 끈다.
///
/// 상대는 `walls` 전부에서 찾는다 — 두께가 다른 벽에 붙는 T 가 흔하다
/// (얇은 칸막이가 두꺼운 외벽에 닿는 것).
pub fn extend_tees(chains: &mut [WallChain], walls: &[WallSeg]) {
    for c in chains.iter_mut() {
        if c.closed || c.pts.len() < 2 {
            continue;
        }
        let r = join_r(c.thickness);
        // 시작 끝: 줄기 **바깥**으로 향하는 방향은 pts[0] - pts[1].
        if c.free_start {
            if let Some(ext) = tee_extension(c.pts[0], c.pts[0] - c.pts[1], walls, r) {
                let dir = (c.pts[0] - c.pts[1]).normalized();
                c.pts[0] = c.pts[0] + dir * ext;
                c.free_start = false;
            }
        }
        if c.free_end {
            let last = c.pts.len() - 1;
            if let Some(ext) = tee_extension(c.pts[last], c.pts[last] - c.pts[last - 1], walls, r) {
                let dir = (c.pts[last] - c.pts[last - 1]).normalized();
                c.pts[last] = c.pts[last] + dir * ext;
                c.free_end = false;
            }
        }
    }
}

/// `tip` 이 어떤 벽의 **몸통**에 닿아 있으면, 그 벽을 건너가는 데 필요한 길이.
fn tee_extension(tip: Vec2, out_dir: Vec2, walls: &[WallSeg], r: f64) -> Option<f64> {
    let u = out_dir.normalized();
    if !u.is_finite() || u == Vec2::ZERO {
        return None;
    }
    let mut best: Option<f64> = None;
    for w in walls {
        if !usable(w) {
            continue;
        }
        let d = w.b - w.a;
        let wlen = d.dist(Vec2::ZERO);
        let wu = d.normalized();
        // 몸통에 닿았는가 — 끝점 근처는 제외한다. 거긴 줄기가 이미 다뤘다.
        let t = (tip - w.a).dot(wu);
        if t <= r || t >= wlen - r {
            continue;
        }
        let foot = w.a + wu * t;
        if tip.dist(foot) > r {
            continue;
        }
        // 나란하면 건너갈 게 없다.
        let sin = (u.x * wu.y - u.y * wu.x).abs();
        if sin <= 0.05 {
            continue;
        }
        let h = w.thickness / 2.0;
        // 아주 비스듬하면 창처럼 뻗는다. `WallShape.tsx` 와 같은 한도로 묶는다.
        let ext = (h / sin).min(h * 8.0);
        best = Some(best.map_or(ext, |b: f64| b.max(ext)));
    }
    best
}

/// 벽 줄기를 MLINE 스타일로.
///
/// 요소는 ±0.5 두 줄이고 `scale` 이 두께다. 그래서 중심선 기준 ±(두께/2) 가
/// 되고, `MLineJust::Zero` 로 **찍은 선이 중심선**이 된다 — Bimove 의 벽이
/// 중심선 기준이기 때문이다. 기본값인 `Top` 을 쓰면 벽이 두께의 절반만큼
/// 한쪽으로 밀린다.
fn style_for(c: &WallChain) -> MLineStyle {
    MLineStyle {
        name: format!("BIMOVE_WALL_{}", (c.thickness * 1000.0).round() as i64),
        elements: vec![
            MLineElement { offset: 0.5, color: c.color, ..Default::default() },
            MLineElement { offset: -0.5, color: c.color, ..Default::default() },
        ],
        // 캡은 **끝마다 따로** 켜고 끄므로 스타일에서는 켜 둔다. 어느 끝을
        // 막을지는 `no_start_caps` / `no_end_caps` 가 정한다.
        start_caps: MLineCaps { line: true, ..Default::default() },
        end_caps: MLineCaps { line: true, ..Default::default() },
        fill: c.fill,
        joints: false,
        ..Default::default()
    }
}

/// 줄기 하나를 MLINE 엔티티로.
pub fn mline_for(c: &WallChain) -> MLine {
    let def = style_for(c).sanitized();
    MLine {
        style: def.name.clone(),
        def,
        scale: c.thickness,
        justification: MLineJust::Zero,
        closed: c.closed,
        no_start_caps: !c.free_start,
        no_end_caps: !c.free_end,
        vertices: c.pts.iter().map(|p| MLineVertex::new(Vec3::new(p.x, p.y, 0.0))).collect(),
    }
}

/// 벽들을 도면에 넣는다. **만든 MLINE 수**를 돌려준다.
///
/// 벽 수가 아니라 MLINE 수다 — 이어진 벽이 하나로 묶이므로 보통 훨씬 적다.
/// 부른 쪽이 둘을 같이 보여주면 묶기가 듣는지 눈으로 확인된다.
pub fn add_walls(drawing: &mut Drawing, walls: &[WallSeg]) -> usize {
    let mut chains = chain_walls(walls);
    extend_tees(&mut chains, walls);
    let mut n = 0;
    for c in &chains {
        if c.pts.len() < 2 {
            continue;
        }
        let h = drawing.new_handle();
        let mut e = Entity::new(h, EntityKind::MLine(mline_for(c)));
        e.common.layer = c.layer.clone();
        e.common.color = c.color;
        drawing.model.push(e);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(ax: f64, ay: f64, bx: f64, by: f64, t: f64) -> WallSeg {
        WallSeg {
            a: Vec2::new(ax, ay),
            b: Vec2::new(bx, by),
            thickness: t,
            layer: "0".into(),
            color: Color::ByLayer,
            fill: None,
        }
    }

    #[test]
    fn 꺾인_두_벽은_한_줄기로_묶인다() {
        // (0,0)→(100,0)→(100,100). 코너는 MLINE 의 꼭짓점이 된다.
        let w = vec![seg(0.0, 0.0, 100.0, 0.0, 10.0), seg(100.0, 0.0, 100.0, 100.0, 10.0)];
        let c = chain_walls(&w);
        assert_eq!(c.len(), 1, "줄기 하나여야 한다: {c:?}");
        assert_eq!(c[0].pts.len(), 3);
        assert!(!c[0].closed);
        // 양 끝은 비어 있다 — 캡을 그린다.
        assert!(c[0].free_start && c[0].free_end);
    }

    #[test]
    fn 들어온_차례가_거꾸로여도_묶는다() {
        let w = vec![seg(100.0, 0.0, 100.0, 100.0, 10.0), seg(0.0, 0.0, 100.0, 0.0, 10.0)];
        let c = chain_walls(&w);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].pts.len(), 3);
    }

    #[test]
    fn 끝점_방향이_뒤집혀_있어도_묶는다() {
        // 둘째 벽이 코너에서 **시작**하지 않고 코너로 **들어온다**.
        let w = vec![seg(0.0, 0.0, 100.0, 0.0, 10.0), seg(100.0, 100.0, 100.0, 0.0, 10.0)];
        let c = chain_walls(&w);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].pts.len(), 3);
    }

    #[test]
    fn 두께가_다르면_못_묶는다() {
        // MLINE 하나는 스타일 하나다.
        let w = vec![seg(0.0, 0.0, 100.0, 0.0, 10.0), seg(100.0, 0.0, 100.0, 100.0, 20.0)];
        let c = chain_walls(&w);
        assert_eq!(c.len(), 2);
        // 만난 끝은 **캡을 끈다** — 캡이 코너를 가로지르는 선이 된다.
        assert!(c.iter().any(|x| !x.free_end || !x.free_start), "한쪽은 막혀 있지 않아야 한다: {c:?}");
    }

    #[test]
    fn 저장_오차만큼_다른_두께는_같게_본다() {
        // 1e-13 어긋난 걸 다른 두께로 보면 줄기가 벽마다 끊어진다.
        let w = vec![seg(0.0, 0.0, 100.0, 0.0, 10.0), seg(100.0, 0.0, 100.0, 100.0, 10.0 + 1e-13)];
        assert_eq!(chain_walls(&w).len(), 1);
    }

    #[test]
    fn 떨어진_벽은_따로_낸다() {
        let w = vec![seg(0.0, 0.0, 100.0, 0.0, 10.0), seg(500.0, 500.0, 600.0, 500.0, 10.0)];
        let c = chain_walls(&w);
        assert_eq!(c.len(), 2);
        assert!(c.iter().all(|x| x.free_start && x.free_end));
    }

    #[test]
    fn 사각형은_닫힌_줄기가_된다() {
        let w = vec![
            seg(0.0, 0.0, 100.0, 0.0, 10.0),
            seg(100.0, 0.0, 100.0, 100.0, 10.0),
            seg(100.0, 100.0, 0.0, 100.0, 10.0),
            seg(0.0, 100.0, 0.0, 0.0, 10.0),
        ];
        let c = chain_walls(&w);
        assert_eq!(c.len(), 1);
        assert!(c[0].closed, "닫혀야 한다: {c:?}");
        // 닫힌 줄기는 마지막 점을 되풀이하지 않는다.
        assert_eq!(c[0].pts.len(), 4);
    }

    #[test]
    fn 벽_셋이_만나면_거기서_끊고_캡을_끈다() {
        // ㅗ 자. 가로 둘 + 위로 하나가 (100,0) 에서 만난다.
        let w = vec![
            seg(0.0, 0.0, 100.0, 0.0, 10.0),
            seg(100.0, 0.0, 200.0, 0.0, 10.0),
            seg(100.0, 0.0, 100.0, 100.0, 10.0),
        ];
        let c = chain_walls(&w);
        assert_eq!(c.len(), 3, "셋이 만난 자리는 묶지 않는다: {c:?}");
        // 만난 끝은 전부 캡이 꺼져야 한다 — 안 끄면 세 줄이 코너에서 겹친다.
        let capped: usize =
            c.iter().map(|x| usize::from(x.free_start) + usize::from(x.free_end)).sum();
        assert_eq!(capped, 3, "비어 있는 끝은 셋뿐이다: {c:?}");
    }

    #[test]
    fn 길이_0_과_nan_은_버린다() {
        let w = vec![
            seg(0.0, 0.0, 0.0, 0.0, 10.0),
            seg(f64::NAN, 0.0, 100.0, 0.0, 10.0),
            seg(0.0, 0.0, 100.0, 0.0, 0.0),
            seg(0.0, 0.0, 100.0, 0.0, 10.0),
        ];
        let c = chain_walls(&w);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].pts, vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)]);
    }

    #[test]
    fn t_자는_상대_벽을_건너가게_밀어낸다() {
        // 가로 벽 (0,0)→(200,0) 두께 100. 세로 칸막이가 그 **가운데**에 닿는다.
        let walls = vec![seg(0.0, 0.0, 200.0, 0.0, 100.0), seg(100.0, 200.0, 100.0, 0.0, 20.0)];
        let mut c = chain_walls(&walls);
        // 두께가 달라 따로 묶인다.
        assert_eq!(c.len(), 2);
        extend_tees(&mut c, &walls);
        let stem = c.iter().find(|x| x.thickness == 20.0).expect("칸막이");
        // 직각이라 sin=1 → 반두께 50 만큼 건너간다. y = 0 - 50.
        let tip = stem.pts.iter().copied().min_by(|p, q| p.y.total_cmp(&q.y)).expect("점");
        assert!((tip.y - -50.0).abs() < 1e-9, "밀어낸 끝이 틀렸다: {tip:?}");
        // 밀어낸 끝은 비어 있지 않다 — 캡을 끈다.
        assert!(!(stem.free_start && stem.free_end), "한쪽 캡은 꺼져야 한다: {stem:?}");
    }

    #[test]
    fn 몸통에_안_닿으면_안_밀어낸다() {
        // 칸막이가 가로 벽에서 멀다.
        let walls = vec![seg(0.0, 0.0, 200.0, 0.0, 100.0), seg(100.0, 200.0, 100.0, 150.0, 20.0)];
        let mut c = chain_walls(&walls);
        let before = c.clone();
        extend_tees(&mut c, &walls);
        assert_eq!(c, before, "건드리지 않아야 한다");
    }

    #[test]
    fn 나란한_벽은_건너갈_게_없다() {
        // 끝이 다른 가로 벽 **몸통 위**에 겹쳐 있지만 방향이 같다.
        let walls = vec![seg(0.0, 0.0, 200.0, 0.0, 100.0), seg(100.0, 0.0, 300.0, 0.0, 20.0)];
        let mut c = chain_walls(&walls);
        let before = c.clone();
        extend_tees(&mut c, &walls);
        assert_eq!(c, before, "나란하면 밀어낼 양이 없다");
    }

    #[test]
    fn mline_은_중심선_기준이고_두께가_scale_이다() {
        let c = &chain_walls(&[seg(0.0, 0.0, 100.0, 0.0, 120.0)])[0];
        let m = mline_for(c);
        assert_eq!(m.justification, MLineJust::Zero, "중심선 기준이어야 한다");
        assert_eq!(m.scale, 120.0);
        // 요소 오프셋 × scale = ±60 → 두께 120.
        let offs = m.offsets();
        assert_eq!(offs.len(), 2);
        let span = offs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - offs.iter().copied().fold(f64::INFINITY, f64::min);
        assert!((span - 120.0).abs() < 1e-9, "두께가 {span} 로 나왔다");
    }

    #[test]
    fn 코너는_miter_로_맞물린다() {
        // 이게 이 파일의 존재 이유다. 90도 코너에서 **바깥 면은 뻗고 안쪽 면은
        // 당겨져야** 한다 — 같은 양을 더하면 안쪽에 꼬다리가 남는다.
        let c = &chain_walls(&[
            seg(0.0, 0.0, 100.0, 0.0, 20.0),
            seg(100.0, 0.0, 100.0, 100.0, 20.0),
        ])[0];
        let m = mline_for(c);
        let mi = m.miters();
        // 코너 꼭짓점(가운데)의 miter 벡터. 45도 이등분이고 길이는 √2 다.
        let corner = mi[1];
        assert!((corner.dist(Vec2::ZERO) - 2f64.sqrt()).abs() < 1e-6, "miter 길이: {corner:?}");
        // 바깥 면과 안쪽 면이 **중심선 반대쪽**으로 갈라진다.
        let outer = Vec2::new(100.0, 0.0) + corner * 10.0;
        let inner = Vec2::new(100.0, 0.0) + corner * -10.0;
        assert!(outer.dist(inner) > 20.0, "두 면이 갈라져야 한다: {outer:?} {inner:?}");
    }

    #[test]
    fn 채움을_주면_스타일에_실린다() {
        let mut w = seg(0.0, 0.0, 100.0, 0.0, 20.0);
        w.fill = Some(Color::WHITE);
        let c = &chain_walls(&[w])[0];
        assert_eq!(mline_for(c).def.fill, Some(Color::WHITE));
    }

    #[test]
    fn 레이어가_다르면_못_묶는다() {
        let mut b = seg(100.0, 0.0, 100.0, 100.0, 10.0);
        b.layer = "벽체2".into();
        let c = chain_walls(&[seg(0.0, 0.0, 100.0, 0.0, 10.0), b]);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn 긴_줄기도_한_번에_묶는다() {
        // 계단식 10 구간. 하나라도 끊기면 코너마다 캡이 생긴다.
        let mut w = Vec::new();
        let mut p = Vec2::new(0.0, 0.0);
        for i in 0..10 {
            let q = if i % 2 == 0 { Vec2::new(p.x + 100.0, p.y) } else { Vec2::new(p.x, p.y + 100.0) };
            w.push(seg(p.x, p.y, q.x, q.y, 10.0));
            p = q;
        }
        let c = chain_walls(&w);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].pts.len(), 11);
    }
}
