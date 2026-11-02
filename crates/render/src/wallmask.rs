//! 겹친 벽을 **한 덩어리로** 보이게 한다.
//!
//! 한: "겹치는거를 한개로 보이게 하는게 핵심이라고"
//!
//! 벽은 저마다 제 외곽선을 전부 긋는다. 그래서 두 벽이 겹치면 서로의 선이
//! 상대 몸통을 **뚫고 지나가** 평면도에 X 가 생긴다 — 한 덩어리가 아니라 겹쳐
//! 놓은 유리판처럼 보인다.
//!
//! 고치는 길은 둘이었다:
//!
//!   1. 몸통들을 합집합(boolean union)해서 그 테두리만 긋기. 맞지만 bool 연산
//!      라이브러리가 필요하고, 선택·스냅·DXF 가 쓰는 `geometry()` 를 갈아야 한다.
//!   2. 기하는 그대로 두고 **그릴 때만** 남의 몸통 안에 든 토막을 빼기.
//!
//! 여기는 2 번이다. 합집합의 테두리는 "어느 몸통 안에도 안 든 테두리 토막"
//! 과 같은 것이므로 **그림은 1 번과 같고**, 바뀌는 것은 화면뿐이다. 벽을
//! 안 보이게 해서 푸는 길도 있었지만 그건 못 쓴다 — `is_visible` 이 선택
//! (`select.rs`)과 스냅(`snap.rs`)을 같이 가른다.
//!
//! 켜는 것은 벽뿐이다 (`MLine::merge_overlaps`). 일반 DXF multiline 은 그대로
//! 제 선을 다 긋는다.

use cadcraft_doc::{Drawing, EntityKind, Handle, MLine, MLineCut, Space};
use cadcraft_geom::Vec2;

/// 몸통 가장자리에 **딱 붙은** 점을 "안"으로 치지 않는 여유.
///
/// 용접된 두 벽은 가장자리를 **공유한다.** 그 선 위의 점은 안이냐 밖이냐가
/// 반올림으로 갈려서, 여유 없이 재면 멀쩡한 바깥 테두리가 군데군데 사라진다.
/// 도면 단위(벽 두께가 보통 100 언저리)에서 이 값은 충분히 작다.
const ON_EDGE_EPS: f64 = 1e-6;

/// 겹침을 합칠 벽 하나.
struct Wall {
    h: Handle,
    lo: Vec2,
    hi: Vec2,
    /// 몸통 고리들. 여럿이면 even-odd 로 읽는다 (닫힌 벽의 가운데 구멍).
    loops: Vec<Vec<Vec2>>,
}

/// 한 공간에서 **합칠 벽 전부.**
pub struct WallMask {
    walls: Vec<Wall>,
}

/// 합치기를 DXF 가 아는 말로 구운 것 — [`WallMask::bake`] 가 낸다.
pub struct Baked {
    /// 꼭짓점마다, **거기서 시작하는 세그먼트**의 끊기 (`MLineVertex::cuts`).
    pub cuts: Vec<Vec<MLineCut>>,
    pub no_start_caps: bool,
    pub no_end_caps: bool,
}

impl WallMask {
    /// `space` 안에서 합치기를 켠 multiline 들을 모은다. 하나뿐이면 겹칠
    /// 상대가 없으니 `None` — 부른 쪽이 통째로 건너뛴다.
    pub fn collect(d: &Drawing, space: &Space) -> Option<WallMask> {
        let mut walls = Vec::new();
        for e in d.space(space)?.iter() {
            let EntityKind::MLine(m) = &e.kind else { continue };
            if !m.merge_overlaps || !d.is_visible(e) {
                continue;
            }
            let loops = m.body();
            let Some((lo, hi)) = bounds(&loops) else { continue };
            walls.push(Wall { h: e.handle, lo, hi, loops });
        }
        (walls.len() > 1).then_some(WallMask { walls })
    }

    /// `e` 의 선 `[a, b]` 에서 **남의 벽 속에 든 토막을 뺀** 나머지.
    ///
    /// 자를 데가 없으면 빈 벡터가 아니라 통째로 하나를 돌려준다 — 흔한 쪽이
    /// 그쪽이라 할당을 아낀다.
    pub fn clip(&self, e: Handle, a: Vec2, b: Vec2) -> Vec<(Vec2, Vec2)> {
        let others: Vec<&Wall> = self.walls.iter().filter(|w| w.h != e && w.overlaps(a, b)).collect();
        if others.is_empty() {
            return vec![(a, b)];
        }
        // 상대 가장자리와 만나는 자리마다 끊는다. 그 사이는 통째로 안이거나
        // 통째로 밖이므로 가운데 한 점만 보면 된다.
        let mut ts = vec![0.0, 1.0];
        for w in &others {
            w.cross_params(a, b, &mut ts);
        }
        ts.sort_by(f64::total_cmp);
        let mut out = Vec::new();
        for pair in ts.windows(2) {
            let (t0, t1) = (pair[0], pair[1]);
            if t1 - t0 <= 1e-12 {
                continue;
            }
            let mid = a + (b - a) * ((t0 + t1) * 0.5);
            if others.iter().any(|w| w.swallows(mid)) {
                continue;
            }
            out.push((a + (b - a) * t0, a + (b - a) * t1));
        }
        out
    }

    /// 합치기를 **DXF 가 아는 말로** 굽는다.
    ///
    /// 한: "그게 오토캐드랑 딱 호환되게끔 내보내기 했었어"
    ///
    /// 화면에서는 그릴 때마다 잘라내면 된다 (`clip`). 파일로 내보내면 그
    /// 자리가 없다 — DXF 에 "겹친 이웃과 합쳐라" 같은 칸은 없어서, 내보낸
    /// 벽은 제 선을 다 긋고 AutoCAD 에서 교차 자리에 X 가 다시 생긴다.
    ///
    /// 그런데 DXF 의 MLINE 은 **요소별 끊기**를 정식으로 안다 (group 73/74/41,
    /// MLEDIT 의 Cut). 잘라낸 토막을 그 끊기로 바꿔 넣으면 AutoCAD 가 우리와
    /// 같은 그림을 그린다 — MLEDIT 의 "Merged Cross/Tee" 와 같은 결과물이다.
    ///
    /// 끊기는 세그먼트 시작점(miter 가 적용된 자리)으로부터의 **거리**다
    /// (`MLineCut`). 그 거리를 재는 선이 곧 `MLine::element_runs` 이고,
    /// `geometry` 도 같은 선에서 끊기를 빼므로 둘이 갈릴 수 없다.
    pub fn bake(&self, e: Handle, m: &MLine) -> Baked {
        let mut cuts = vec![Vec::new(); m.vertices.len()];
        for (k, el, a, b) in m.element_runs() {
            let len = a.dist(b);
            // 남는(= 남의 몸통 밖) 토막들. 자를 데가 없으면 통째로 하나다.
            let kept = self.clip(e, a, b);
            let mut from = 0.0;
            for (s, t) in &kept {
                let (ds, dt) = (a.dist(*s), a.dist(*t));
                if ds > from + ON_EDGE_EPS {
                    cuts[k].push(MLineCut { element: el, start: from, end: ds });
                }
                from = f64::max(from, dt);
            }
            if len > from + ON_EDGE_EPS {
                cuts[k].push(MLineCut { element: el, start: from, end: len });
            }
        }
        // 끝막음선은 **통째로** 끄거나 켜는 수밖에 없다 — DXF 가 캡을 부분적
        // 으로 끊지 못한다. 그래서 그 선이 남의 몸통에 완전히 묻혔을 때만
        // 끈다. T 자로 만나는 자리가 그렇다: 가로벽 끝의 막음선이 세로벽
        // 속으로 들어가 덩어리 한가운데를 가로지른다.
        let (mut no_start, mut no_end) = (m.no_start_caps, m.no_end_caps);
        if !m.closed {
            if let Some((a, b)) = self.cap_line(m, true) {
                no_start |= self.clip(e, a, b).is_empty();
            }
            if let Some((a, b)) = self.cap_line(m, false) {
                no_end |= self.clip(e, a, b).is_empty();
            }
        }
        Baked { cuts, no_start_caps: no_start, no_end_caps: no_end }
    }

    /// 첫(`start`) 또는 마지막 세그먼트의 **끝막음선** — 가장 바깥 요소 둘을 잇는다.
    fn cap_line(&self, m: &MLine, start: bool) -> Option<(Vec2, Vec2)> {
        let offs = m.offsets();
        let hi = (0..offs.len()).max_by(|a, b| offs[*a].total_cmp(&offs[*b]))?;
        let lo = (0..offs.len()).min_by(|a, b| offs[*a].total_cmp(&offs[*b]))?;
        if hi == lo {
            return None; // 요소가 하나뿐이면 막을 것이 없다.
        }
        let runs = m.element_runs();
        let k = if start { 0 } else { m.segment_count().checked_sub(1)? };
        let end = |el: usize| {
            runs.iter().find(|(kk, ee, _, _)| *kk == k && *ee == el).map(|(_, _, a, b)| if start { *a } else { *b })
        };
        Some((end(hi)?, end(lo)?))
    }

    /// 여러 점으로 된 선(캡·호)을 토막마다 잘라 **이어지는 조각들**로.
    pub fn clip_path(&self, e: Handle, pts: &[Vec2]) -> Vec<Vec<Vec2>> {
        let mut out: Vec<Vec<Vec2>> = Vec::new();
        for ab in pts.windows(2) {
            for (a, b) in self.clip(e, ab[0], ab[1]) {
                match out.last_mut() {
                    // 앞 조각의 끝에서 이어지면 같은 조각으로 붙인다.
                    Some(run) if run.last().is_some_and(|p| p.dist(a) <= ON_EDGE_EPS) => run.push(b),
                    _ => out.push(vec![a, b]),
                }
            }
        }
        out
    }
}

impl Wall {
    /// 선분의 테두리 상자가 이 벽과 스칠 수라도 있는가 — 싼 거르개다.
    fn overlaps(&self, a: Vec2, b: Vec2) -> bool {
        a.x.min(b.x) <= self.hi.x && a.x.max(b.x) >= self.lo.x && a.y.min(b.y) <= self.hi.y && a.y.max(b.y) >= self.lo.y
    }

    /// `[a, b]` 가 이 벽의 가장자리를 지나는 매개변수들을 `ts` 에 담는다.
    fn cross_params(&self, a: Vec2, b: Vec2, ts: &mut Vec<f64>) {
        let d = b - a;
        for ring in &self.loops {
            for i in 0..ring.len() {
                let (p, q) = (ring[i], ring[(i + 1) % ring.len()]);
                let e = q - p;
                let den = d.x * e.y - d.y * e.x;
                if den.abs() < 1e-12 {
                    continue; // 나란하다 — 끊을 자리가 없다.
                }
                let w = p - a;
                let t = (w.x * e.y - w.y * e.x) / den;
                let u = (w.x * d.y - w.y * d.x) / den;
                if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                    ts.push(t);
                }
            }
        }
    }

    /// `p` 가 이 벽 **속**에 들었는가 — 가장자리에 붙은 점은 아니다.
    fn swallows(&self, p: Vec2) -> bool {
        if p.x < self.lo.x || p.x > self.hi.x || p.y < self.lo.y || p.y > self.hi.y {
            return false;
        }
        let mut inside = false;
        for ring in &self.loops {
            for i in 0..ring.len() {
                let (c, q) = (ring[i], ring[(i + 1) % ring.len()]);
                // 가장자리를 베고 누운 점은 **안이 아니다** — 용접된 이웃끼리
                // 가장자리를 공유하므로, 여기서 안이라고 하면 바깥 테두리가
                // 서로를 지운다.
                if dist_to_seg(p, c, q) <= ON_EDGE_EPS {
                    return false;
                }
                if (c.y > p.y) != (q.y > p.y) && p.x < (q.x - c.x) * (p.y - c.y) / (q.y - c.y) + c.x {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

fn bounds(loops: &[Vec<Vec2>]) -> Option<(Vec2, Vec2)> {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in loops.iter().flatten() {
        if !p.is_finite() {
            return None;
        }
        lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    lo.is_finite().then_some((lo, hi))
}

fn dist_to_seg(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let len2 = d.dot(d);
    if len2 <= 1e-24 {
        return p.dist(a);
    }
    let t = ((p - a).dot(d) / len2).clamp(0.0, 1.0);
    p.dist(a + d * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{Entity, EntityKind, MLine, MLineElement, MLineJust, MLineStyle};

    /// 중심선 `a→b`, 두께 `t` 짜리 벽 하나. 빔모바가 만드는 것과 같은 꼴이다
    /// (요소 ±0.5, `scale` 이 두께, `Zero` 기준).
    pub(super) fn wall(h: u64, a: (f64, f64), b: (f64, f64), t: f64) -> Entity {
        let def = MLineStyle {
            name: "W".into(),
            elements: vec![
                MLineElement { offset: 0.5, ..Default::default() },
                MLineElement { offset: -0.5, ..Default::default() },
            ],
            ..Default::default()
        };
        let mut m = MLine::new(&def, &[Vec2::new(a.0, a.1), Vec2::new(b.0, b.1)], false, MLineJust::Zero, t);
        m.merge_overlaps = true;
        Entity::new(Handle(h), EntityKind::MLine(m))
    }

    /// 벽들을 도면에 넣고 마스크를 만든다.
    fn mask(ents: Vec<Entity>) -> Option<WallMask> {
        let mut d = Drawing::new_metric();
        for e in ents {
            d.model.push(e);
        }
        WallMask::collect(&d, &Space::Model)
    }

    /// T 자로 만나는 벽 둘 — 가로벽 (0,0)→(3000,0), 세로벽 (1500,±1500),
    /// 둘 다 두께 250. 세로벽의 몸통은 x ∈ [1375, 1625] 이므로 가로벽의
    /// 외곽선은 **딱 그 구간에서** 끊겨야 한다.
    fn tee() -> (Drawing, Handle) {
        let mut d = Drawing::new_metric();
        d.model.push(wall(1, (0.0, 0.0), (3000.0, 0.0), 250.0));
        d.model.push(wall(2, (1500.0, -1500.0), (1500.0, 1500.0), 250.0));
        (d, Handle(1))
    }

    /// **끊기가 교차 구간에 정확히 생긴다.**
    ///
    /// 한: "내보내기도 ... 오토캐드랑 딱 호환되게끔"
    ///
    /// 화면의 합치기는 그릴 때만 잘라내므로 기하에 안 남는다. 파일로 내보내면
    /// 그 자리가 없어서 AutoCAD 에서 교차 자리에 X 가 다시 생겼다. 잘라냄을
    /// MLINE 의 요소별 끊기로 구워 그걸 막는다 (`bake`).
    #[test]
    fn 끊기가_남의_몸통_구간에_생긴다() {
        let (d, h) = tee();
        let mask = WallMask::collect(&d, &Space::Model).expect("벽 둘");
        let EntityKind::MLine(m) = &d.model.get(h).unwrap().kind else { panic!() };
        let baked = mask.bake(h, m);

        // 가로벽은 세그먼트 하나뿐이라 끊기는 첫 꼭짓점에 달린다.
        let cuts = &baked.cuts[0];
        assert_eq!(cuts.len(), 2, "외곽선 둘이 각각 한 번 끊겨야 한다: {cuts:?}");
        for c in cuts {
            assert!((c.start - 1375.0).abs() < 1e-6, "끊기 시작이 1375 가 아니다: {c:?}");
            assert!((c.end - 1625.0).abs() < 1e-6, "끊기 끝이 1625 가 아니다: {c:?}");
        }
        // 가로벽 끝은 세로벽 밖이라 막음선은 **그대로 둔다.**
        assert!(!baked.no_start_caps && !baked.no_end_caps, "엉뚱한 캡을 껐다");
    }

    /// **구운 끊기가 화면에서 자른 것과 같다.**
    ///
    /// 둘이 갈리면 화면과 내보낸 파일이 다른 그림이 된다 — 이 수정이 막으려는
    /// 바로 그 일이다. 그래서 같은 벽을 두 길로 그려 길이를 견준다.
    #[test]
    fn 구운_것과_화면에서_자른_것이_같다() {
        let (mut d, h) = tee();
        let mask = WallMask::collect(&d, &Space::Model).expect("벽 둘");
        let EntityKind::MLine(m) = &d.model.get(h).unwrap().kind else { panic!() };

        // 화면: geometry 의 선마다 마스크를 건다.
        let on_screen: f64 = m
            .geometry()
            .lines
            .iter()
            .flat_map(|(_, a, b)| mask.clip(h, *a, *b))
            .map(|(a, b)| a.dist(b))
            .sum();

        // 파일: 끊기를 구워 넣고 geometry 가 알아서 빼게 한다.
        let baked = mask.bake(h, m);
        d.model.modify(h, |e| {
            let EntityKind::MLine(m) = &mut e.kind else { return };
            for (v, cuts) in m.vertices.iter_mut().zip(baked.cuts) {
                v.cuts.extend(cuts);
            }
        });
        let EntityKind::MLine(m) = &d.model.get(h).unwrap().kind else { panic!() };
        let in_file: f64 = m.geometry().lines.iter().map(|(_, a, b)| a.dist(*b)).sum();

        assert!((on_screen - in_file).abs() < 1e-6, "화면 {on_screen} vs 파일 {in_file}");
        // 자르기 전보다 **짧아야** 한다 — 안 짧으면 아무것도 안 뺀 것이다.
        assert!(in_file < 2.0 * 3000.0 - 1.0, "끊긴 자리가 없다: {in_file}");
    }

    #[test]
    fn 벽이_하나면_합칠_것이_없다() {
        assert!(mask(vec![wall(1, (0.0, 0.0), (100.0, 0.0), 20.0)]).is_none());
    }

    #[test]
    fn 합치기를_안_켠_multiline_은_모으지_않는다() {
        // 일반 DXF multiline 은 여태처럼 제 선을 다 긋는다.
        let mut a = wall(1, (0.0, 0.0), (100.0, 0.0), 20.0);
        let mut b = wall(2, (0.0, 0.0), (0.0, 100.0), 20.0);
        for e in [&mut a, &mut b] {
            if let EntityKind::MLine(m) = &mut e.kind {
                m.merge_overlaps = false;
            }
        }
        assert!(mask(vec![a, b]).is_none());
    }

    #[test]
    fn 멀리_떨어진_벽은_아무것도_안_자른다() {
        let m = mask(vec![wall(1, (0.0, 0.0), (100.0, 0.0), 20.0), wall(2, (0.0, 500.0), (100.0, 500.0), 20.0)]).unwrap();
        let (a, z) = (Vec2::new(0.0, 10.0), Vec2::new(100.0, 10.0));
        assert_eq!(m.clip(Handle(1), a, z), vec![(a, z)]);
    }

    #[test]
    fn 남의_벽_속에_통째로_든_선은_사라진다() {
        // 2 번 벽은 x 0..200 · y -50..50 을 덮는다. 그 한가운데를 긋는다.
        let m = mask(vec![wall(1, (0.0, 0.0), (10.0, 0.0), 2.0), wall(2, (0.0, 0.0), (200.0, 0.0), 100.0)]).unwrap();
        assert!(m.clip(Handle(1), Vec2::new(50.0, 10.0), Vec2::new(150.0, 10.0)).is_empty());
    }

    #[test]
    fn 반만_걸치면_바깥쪽_반만_남는다() {
        let m = mask(vec![wall(1, (0.0, 0.0), (10.0, 0.0), 2.0), wall(2, (0.0, 0.0), (100.0, 0.0), 100.0)]).unwrap();
        // 2 번은 x 0..100. x 50 에서 150 까지 그으면 100 부터만 남아야 한다.
        let got = m.clip(Handle(1), Vec2::new(50.0, 10.0), Vec2::new(150.0, 10.0));
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0].0.x - 100.0).abs() < 1e-9, "{got:?}");
        assert!((got[0].1.x - 150.0).abs() < 1e-9, "{got:?}");
    }

    #[test]
    fn 제_몸통은_제가_안_지운다() {
        // 벽은 제 외곽선 안을 지나지 않지만, 혹시라도 제 자신을 상대로
        // 세면 벽이 통째로 사라진다. 손잡이로 거르는지 본다.
        let m = mask(vec![wall(1, (0.0, 0.0), (100.0, 0.0), 100.0), wall(2, (0.0, 300.0), (100.0, 300.0), 20.0)]).unwrap();
        let (a, z) = (Vec2::new(10.0, 0.0), Vec2::new(90.0, 0.0));
        assert_eq!(m.clip(Handle(1), a, z), vec![(a, z)], "제 몸통 속이라고 제 선을 지웠다");
    }

    #[test]
    fn 가장자리를_맞댄_이웃은_서로의_테두리를_안_지운다() {
        // 용접된 두 벽은 가장자리를 **공유한다.** 그 선을 "안" 으로 세면
        // 멀쩡한 바깥 테두리가 사라진다 (`ON_EDGE_EPS` 가 막는 것).
        let m = mask(vec![wall(1, (0.0, 0.0), (100.0, 0.0), 100.0), wall(2, (100.0, 0.0), (200.0, 0.0), 100.0)]).unwrap();
        // 1 번의 윗 테두리 y = 50, x 0..100. 2 번의 윗 테두리와 한 줄이다.
        let (a, z) = (Vec2::new(0.0, 50.0), Vec2::new(100.0, 50.0));
        assert_eq!(m.clip(Handle(1), a, z), vec![(a, z)], "맞댄 이웃이 테두리를 지웠다");
    }

    /// 한이 보낸 그림 그대로 — **가로지르는** 벽 둘.
    ///
    /// 앞의 시험들은 내가 손으로 고른 선 하나를 본다. 여기서는 벽의 **외곽선
    /// 전체**를 돌려서 "정말 뭔가 잘리긴 하는가" 를 본다 — 배선이 어디 한
    /// 군데라도 끊겼으면 여기서 걸린다 (실제로 한 번 걸렸다).
    #[test]
    fn 가로지르는_두_벽은_속선이_잘린다() {
        let m = mask(vec![
            wall(1, (200.0, 500.0), (2900.0, 380.0), 60.0),
            wall(2, (300.0, 470.0), (2900.0, 640.0), 60.0),
        ])
        .expect("마스크가 안 모였다");
        let EntityKind::MLine(one) = &wall(1, (200.0, 500.0), (2900.0, 380.0), 60.0).kind else { unreachable!() };
        let ring = one.body().remove(0);
        let pieces: usize = ring.windows(2).map(|ab| m.clip(Handle(1), ab[0], ab[1]).len()).sum();
        assert!(pieces > ring.len() - 1, "아무것도 안 잘렸다 (토막 {pieces} / 변 {}）", ring.len() - 1);
    }

    #[test]
    fn 여러_점_선도_이어진_채로_잘린다() {
        let m = mask(vec![wall(1, (0.0, 0.0), (10.0, 0.0), 2.0), wall(2, (0.0, 0.0), (100.0, 0.0), 100.0)]).unwrap();
        // x 150 → 50 → ... 2 번(x 0..100) 안으로 들어가는 꺾은선.
        let pts = [Vec2::new(150.0, 10.0), Vec2::new(120.0, 10.0), Vec2::new(50.0, 10.0)];
        let runs = m.clip_path(Handle(1), &pts);
        assert_eq!(runs.len(), 1, "끊기지 않은 바깥쪽은 한 조각이어야 한다: {runs:?}");
        assert_eq!(runs[0].len(), 3, "꺾인 점이 살아 있어야 한다: {runs:?}");
        assert!((runs[0][2].x - 100.0).abs() < 1e-9, "{runs:?}");
    }
}


/// 그리기까지 **정말 닿는지** 본다.
///
/// 마스크가 맞게 자르는 것과, 그 자른 것이 화면에 가는 것은 다른 얘기다.
/// 한 번 갈렸다 — 마스크는 다 맞는데 그림은 그대로였다. 그래서 여기서는
/// `build` 를 통째로 통과시켜 **그려진 선의 수**를 센다.
#[cfg(test)]
mod wired {
    use super::tests::wall;
    use crate::{Options, build};
    use cadcraft_doc::{Drawing, EntityKind, Space};

    fn lines(merge: bool) -> usize {
        let mut d = Drawing::new_metric();
        for mut e in [wall(1, (200.0, 500.0), (2900.0, 380.0), 60.0), wall(2, (300.0, 470.0), (2900.0, 640.0), 60.0)] {
            if let EntityKind::MLine(m) = &mut e.kind {
                m.merge_overlaps = merge;
            }
            d.model.push(e);
        }
        build(&d, &Space::Model, &Options::default()).prims.len()
    }

    #[test]
    fn 합치기를_켜면_그려지는_선이_달라진다() {
        let (off, on) = (lines(false), lines(true));
        assert_ne!(off, on, "합치기를 켜도 그림이 그대로다 — 배선이 안 닿았다 ({off} 개)");
    }
}

/// 벽이 많아도 **느려지지 않는가.**
///
/// 자르기는 벽 하나당 다른 벽 전부를 훑는다. 테두리 상자로 먼저 거르지만
/// (`overlaps`), 그 거르개가 빠지면 벽 수의 제곱으로 자란다 — 300 벽짜리
/// 도면에서 바로 티가 난다. 그리기는 편집마다 도는 길이라 여기가 느려지면
/// 화면 전체가 느려진다.
#[cfg(test)]
mod speed {
    use super::tests::wall;
    use crate::{Options, build};
    use cadcraft_doc::{Drawing, EntityKind, Space};

    #[test]
    fn 벽_사백_개를_그려도_합치기가_짐이_안_된다() {
        let make = |merge: bool| {
            let mut d = Drawing::new_metric();
            for i in 0..400u64 {
                // 격자로 깔되 줄마다 조금씩 어긋내 **진짜 겹치는** 짝을 만든다.
                let (r, c) = ((i / 20) as f64, (i % 20) as f64);
                let y = r * 300.0;
                let x = c * 300.0 + if i % 3 == 0 { 120.0 } else { 0.0 };
                let mut e = wall(i + 1, (x, y), (x + 400.0, y + 40.0), 100.0);
                if let EntityKind::MLine(m) = &mut e.kind {
                    m.merge_overlaps = merge;
                }
                d.model.push(e);
            }
            d
        };
        let run = |d: &Drawing| {
            let t = std::time::Instant::now();
            let n = build(d, &Space::Model, &Options::default()).prims.len();
            (t.elapsed(), n)
        };
        let (off, n_off) = run(&make(false));
        let (on, n_on) = run(&make(true));
        println!("합치기 끔 {off:?} ({n_off} 선) · 켬 {on:?} ({n_on} 선)");
        // 선 **개수**로는 겹침을 못 본다 — 반만 잘린 선도 여전히 한 줄이다.
        // 자르는 게 듣는지는 `wired` 가 본다. 여기는 값만 잰다.
        //
        // 네 배까지 봐준다 (실측 1.8 배). 상자 거르개가 빠져 제곱으로 자라면
        // 여기서 몇 백 배가 되므로 넉넉히 잡아도 걸린다.
        assert!(on < off * 4 + std::time::Duration::from_millis(50), "합치기가 너무 비싸다: {off:?} → {on:?}");
    }
}
