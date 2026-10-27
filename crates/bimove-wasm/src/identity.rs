//! Bimove 객체의 **정체성**을 엔티티에 붙인다.
//!
//! ── 무엇이 문제였나 ──
//!
//! 문 하나는 엔티티 다섯이 된다 (가림 + 잼브 둘 + 문짝 + 호). 공간 하나는
//! 폴리라인 + 해치 + 글자로 흩어진다. 벽은 거꾸로 **여럿이 하나**가 된다 —
//! 이어진 벽들이 MLINE 한 줄기로 묶인다.
//!
//! 그 상태로는 "문" 이라는 것이 도면 안에 없다. 되돌리기만 묶여 있고
//! (`undo {mode:begin|end}`), 그리고 나면 문짝 선 하나만 골라 지울 수 있다.
//! 물량표가 "문 3개" 를 되짚을 길도 없다 — 선을 세어 셋으로 나눌 수는 없다.
//!
//! ── 왜 xdata 인가 ──
//!
//! 고른 길은 **엔진에 메타데이터를 심는 것**이다. 다른 둘은 이렇다:
//!
//!   블록/그룹 하나로 만든다
//!     선택은 묶이지만 **안을 못 고친다.** 치수 글자를 옮기거나 공간 꼭짓점을
//!     끌려면 블록을 풀어야 하고, 풀면 정체성이 다시 사라진다.
//!   CADCraft 문서를 정본으로 두고 도메인 객체를 따로 직렬화한다
//!     저장 형식이 두 벌이 되어 **늘 어긋난다.** DXF 왕복 시험에서 페이지
//!     3→1 과 오버레이 그림이 구조적으로 날아가는 것을 이미 봤다.
//!
//! xdata 는 객체에 **붙어 다닌다** — 복사·거울·배열·붙여넣기를 따라가고,
//! 되돌리기에 들어가고, DXF 로 나가서 돌아온다 (`cmd/xdata.rs`). 안을 고치는
//! 것도 막지 않는다. 글자를 옮겨도 그 글자는 여전히 그 치수의 것이다.
//!
//! ── 꼴 ──
//!
//! ```json
//! { "kind": "door", "ids": ["shape:abc123"] }
//! ```
//!
//! `ids` 가 **배열인 이유**는 벽이다. 이어진 벽 n 개가 MLINE 하나로 묶이므로
//! 한 엔티티가 Bimove 도형 여럿의 것이 된다. 하나짜리를 `id`, 여럿을 `ids` 로
//! 나눠 두면 읽는 쪽이 매번 두 갈래를 봐야 하고, 한쪽을 빼먹는 날이 온다.
//!
//! 키를 더 넣어도 된다 — 읽는 쪽은 `kind` 와 `ids` 만 보고 나머지는 그대로
//! 둔다. 마감재·단가처럼 뒤에 붙을 것들이 여기로 온다.
//!
//! ── 쓰는 쪽과 맞춰야 한다 ──
//!
//! Bimove 화면에서 **그려 넣는** 문·창도 같은 것을 붙여야 한다
//! (`src/lib/cadIdentity.ts`). 다르면 저장본에서 온 문과 방금 그린 문이
//! 다른 종류가 되고, 그건 "엔진이 저장을 든다" 는 길에서 제일 먼저 드러나는
//! 종류의 어긋남이다.

use cadcraft_doc::{Drawing, Entity};
use cadcraft_engine::cmd::xdata;
use serde_json::{Value, json};

/// 종류 이름 — `src/lib/cadIdentity.ts` 의 `Kind` 와 같아야 한다.
pub const WALL: &str = "wall";
pub const DOOR: &str = "door";
pub const WINDOW: &str = "window";
pub const ZONE: &str = "zone";
pub const DIM: &str = "dim";
pub const BOQ: &str = "boq";

/// 엔티티에서 읽어낸 정체성.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub kind: String,
    /// 이 엔티티가 속한 Bimove 도형 id. 보통 하나, 묶인 벽은 여럿.
    pub ids: Vec<String>,
}

/// `drawing.model[from..]` 전부에 같은 정체성을 붙인다.
///
/// 자리 범위로 찍는 이유는, 문 하나가 엔티티 다섯이 되는 자리마다 손잡이를
/// 들고 다니지 않아도 되기 때문이다. 부르는 쪽은 넣기 **전**의 길이를 재
/// 두었다가 넣은 **뒤**에 한 번 찍는다.
///
/// `ids` 가 비어 있으면 아무것도 안 한다 — 정체성 없는 정체성을 심으면
/// 읽는 쪽이 "묶을 게 있다" 고 보고 빈 묶음을 고른다.
pub fn stamp(drawing: &mut Drawing, from: usize, kind: &str, ids: &[String]) {
    if ids.is_empty() || from >= drawing.model.len() {
        return;
    }
    let data = json!({ "kind": kind, "ids": ids });
    let tags = xdata::encode(xdata::DEFAULT_APP, &data);
    // `EntityStore` 는 자리로 못 집는다 (copy-on-write 라 손잡이로만 고친다).
    // 손잡이를 먼저 모아 두는 이유는 그거다 — 고치는 동안은 빌릴 수 없다.
    let hs: Vec<_> = drawing.model.iter().skip(from).map(|e| e.handle).collect();
    for h in hs {
        drawing.model.modify(h, |e| {
            e.common.xdata = xdata::splice(&e.common.xdata, xdata::DEFAULT_APP, tags.clone());
        });
    }
}

/// 하나짜리 — `stamp` 에 id 하나를 넘기는 흔한 경우.
pub fn stamp_one(drawing: &mut Drawing, from: usize, kind: &str, id: &str) {
    if id.is_empty() {
        return;
    }
    stamp(drawing, from, kind, &[id.to_string()]);
}

/// 엔티티의 정체성. 우리 게 아니거나 꼴이 다르면 `None`.
///
/// 꼴이 다른 걸 억지로 읽지 않는다 — 다른 프로그램이 `BIMOVE` 라는 이름을
/// 쓰고 있을 수도 있고, 우리 옛 꼴일 수도 있다. 둘 다 "정체성 없음" 으로
/// 두는 게 틀린 묶음을 만드는 것보다 낫다.
pub fn of(e: &Entity) -> Option<Identity> {
    let v = xdata::decode(&e.common.xdata, xdata::DEFAULT_APP)?;
    let kind = v.get("kind")?.as_str()?;
    let ids: Vec<String> = v
        .get("ids")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if kind.is_empty() || ids.is_empty() {
        return None;
    }
    Some(Identity { kind: kind.to_string(), ids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_doc::{EntityKind, Line};
    use cadcraft_geom::Vec3;

    fn line_at(d: &mut Drawing, x: f64) {
        let h = d.new_handle();
        d.model.push(Entity::new(
            h,
            EntityKind::Line(Line { a: Vec3::new(x, 0.0, 0.0), b: Vec3::new(x + 1.0, 0.0, 0.0) }),
        ));
    }

    /// 자리로 집을 수 없으므로 그리는 차례로 훑는다.
    fn nth(d: &Drawing, i: usize) -> &Entity {
        d.model.iter().nth(i).expect("엔티티가 없다")
    }

    #[test]
    fn 나중에_넣은_것만_찍는다() {
        // 문 하나를 넣기 전의 선은 남의 것이다. 거기까지 찍으면 그 선이
        // 문짝이 되어 같이 선택되고 같이 지워진다.
        let mut d = Drawing::default();
        line_at(&mut d, 0.0);
        let from = d.model.len();
        line_at(&mut d, 10.0);
        line_at(&mut d, 20.0);

        stamp_one(&mut d, from, DOOR, "shape:a");
        assert_eq!(of(nth(&d, 0)), None, "앞의 선까지 찍혔다");
        for e in d.model.iter().skip(from) {
            assert_eq!(
                of(e),
                Some(Identity { kind: DOOR.into(), ids: vec!["shape:a".into()] }),
            );
        }
    }

    #[test]
    fn 묶인_벽은_id_가_여럿이다() {
        // 이어진 벽 셋이 MLINE 하나가 된다. 그 하나는 도형 셋의 것이다 —
        // 하나만 적으면 나머지 둘은 물량에서 빠진다.
        let mut d = Drawing::default();
        let from = d.model.len();
        line_at(&mut d, 0.0);
        let ids = vec!["shape:a".to_string(), "shape:b".to_string(), "shape:c".to_string()];
        stamp(&mut d, from, WALL, &ids);
        assert_eq!(of(nth(&d, 0)).unwrap().ids, ids);
    }

    #[test]
    fn 빈_것은_안_찍는다() {
        let mut d = Drawing::default();
        line_at(&mut d, 0.0);
        stamp(&mut d, 0, WALL, &[]);
        stamp_one(&mut d, 0, WALL, "");
        assert!(nth(&d, 0).common.xdata.is_empty(), "빈 정체성이 심어졌다");
    }

    #[test]
    fn 꼴이_다르면_없는_것으로_본다() {
        // `BIMOVE` 라는 이름을 다른 프로그램이 쓰고 있을 수도 있다. 억지로
        // 읽어 묶으면 남의 선이 우리 문에 들어온다.
        let mut d = Drawing::default();
        line_at(&mut d, 0.0);
        let junk = xdata::encode(xdata::DEFAULT_APP, &json!({ "whatever": 1 }));
        let h = nth(&d, 0).handle;
        d.model.modify(h, |e| e.common.xdata = junk);
        assert_eq!(of(nth(&d, 0)), None);
    }

    #[test]
    fn 남의_xdata_는_그대로_둔다() {
        let mut d = Drawing::default();
        line_at(&mut d, 0.0);
        let theirs = xdata::encode("ACAD", &json!({ "theirs": true }));
        let h = nth(&d, 0).handle;
        d.model.modify(h, |e| e.common.xdata = theirs);

        stamp_one(&mut d, 0, DOOR, "shape:a");
        assert_eq!(
            xdata::decode(&nth(&d, 0).common.xdata, "ACAD"),
            Some(json!({ "theirs": true })),
            "남의 데이터가 사라졌다",
        );
        assert_eq!(of(nth(&d, 0)).unwrap().kind, DOOR);
    }
}
