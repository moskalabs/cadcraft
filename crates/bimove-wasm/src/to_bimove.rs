//! 엔진 `Drawing` 을 **Bimove 문서로 되돌린다** (`bimove.rs` 의 역방향).
//!
//! ── 왜 이게 없으면 안 되나 ──
//!
//! CadCraft 화면은 읽고·그리고·되돌리기까지 다 되는데 **저장이 없었다.** 그린
//! 것이 저장본으로 돌아가지 못해서 페이지만 바꿔도 사라진다. 그래서 메인
//! 캔버스를 tldraw 에서 넘겨받을 수가 없었다 — 넘기면 작업이 날아간다.
//!
//! ── 원본을 받아서 돌려준다 ──
//!
//! 엔진이 아는 것만으로 문서를 **새로 쓰지 않는다.** 저장본에는 엔진이 모르는
//! 것이 섞여 있다 — 밑그림 `image` 와 그 asset, tldraw `geo`/`arrow`/`text`,
//! 다른 페이지의 도형, 아직 없는 미래의 타입. 새로 쓰면 그게 전부 날아간다.
//!
//! 그래서 `bimove.rs` 가 `props` 를 통째로 싣는 것과 **같은 원리**로 간다:
//! 원본 json 을 받아서, 우리가 책임지는 도형만 손보고 나머지는 **손 안 대고
//! 통과시킨다.** 모르는 것을 지우지 않는 게 유실을 막는 길이다.
//!
//! ── 지울 수 있는 것은 "엔진에 들어갔던 것" 뿐이다 ──
//!
//! 비교를 `drawing` 에 있는 정체성만으로 하면 **안 된다.** 정방향은 도형을
//! 조용히 빼는 자리가 여럿이다: 돌아간 그룹 안, 끊긴 부모 사슬, 두께 0 인 벽,
//! 점이 셋 미만인 공간, 그리고 애초에 다른 페이지. 그 도형들은 도면에 정체성이
//! 없으므로 "사용자가 지웠다" 와 구분이 안 된다 — 그대로 비교하면 **열지도
//! 않은 도형이 저장할 때마다 사라진다.**
//!
//! 그래서 불러온 직후의 id 집합(`loaded`)을 들고 있다가 그것과만 견준다
//! ([`ids_in`] 으로 뜬다, `Doc::from_bimove` 가 보관한다). 모르는 id 는
//! **통과**다. 지우는 쪽으로 기울지 않는 게 이 파일의 유일한 안전장치다.
//!
//! ── 셋뿐이다 ──
//!
//! | 원본 | 도면 | 결과 |
//! |---|---|---|
//! | 있다 | 있다 | **그대로** — 원본 `props` 를 글자 하나 안 고치고 둔다 |
//! | 있다 | 없다 | **없어짐** — 지웠다. 도형을 뺀다 |
//! | 없다 | 있다 | **새로 생김** — 엔티티에서 되읽어 도형을 만든다 |
//!
//! "그대로" 가 **진짜 그대로**인 게 1차의 범위다. 엔진에서 공간을 옮기거나
//! 꼭짓점을 끌어도 `props` 에 안 돌아온다. 되읽기가 손실 없이 되는지를 먼저
//! 확인해야 하고, 그게 되기 전에 원본을 덮어쓰면 그 손실이 저장본에 남는다.
//!
//! ── 정체성 없는 선은 안 건드린다 ──
//!
//! `dxfgroup` 의 선들은 정체성이 없다 (`cadIdentity.ts` 의 `kindForTool` 이
//! `line` 에 `null` 을 주는 것과 같은 결정 — 선 하나는 Bimove 객체가 아니다).
//! 그 선을 엔진에서 지워도 여기서는 안 보이고, `pathData` 를 다시 쓰는 일은
//! 유실 위험이 커서 따로 떼어야 한다. 지금은 `dxfgroup` 을 통째로 통과시킨다.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Entity, EntityKind};
use cadcraft_geom::Vec2;
use serde_json::{Map, Value, json};

use crate::dim::DimScale;
use crate::identity;

/// 되돌린 결과.
pub struct SaveBack {
    /// 저장본으로 그대로 쓸 JSON.
    pub json: String,
    /// 되읽지 못한 것. 사람이 읽는 문장이다 — 조용히 빼면 아무도 모른다.
    pub notes: Vec<String>,
    /// 원본 그대로 둔 도형 수 (엔진이 들고 있던 것 중).
    pub kept: usize,
    /// 지워져서 뺀 도형 수.
    pub erased: usize,
    /// 새로 만들어 넣은 도형 수.
    pub added: usize,
}

/// `kind` → Bimove 도형 `type`. 대부분 같고 둘만 다르다.
fn type_of(kind: &str) -> Option<&'static str> {
    match kind {
        identity::WALL => Some("wall"),
        identity::DOOR => Some("door"),
        identity::WINDOW => Some("window"),
        identity::ZONE => Some("zone"),
        identity::DIM => Some("dimension"),
        identity::BOQ => Some("boqtable"),
        // 그냥 선은 **들여온 도면과 같은 자리**로 간다. `dxfgroup` 은 이름만
        // 그룹이고 속은 `pathData` 한 줄이라, 선 묶음을 담는 유일한 타입이다.
        identity::LINE => Some("dxfgroup"),
        _ => None,
    }
}

/// 도면에 정체성이 박혀 있는 id 전부.
///
/// 불러온 직후에 한 번 떠서 들고 있는다 (모듈 주석의 `loaded`). 저장할 때
/// 다시 뜬 것과 견주면 없어진 것이 나온다.
pub fn ids_in(drawing: &Drawing) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for e in drawing.model.iter() {
        if let Some(id) = identity::of(e) {
            out.extend(id.ids);
        }
    }
    out
}

/// id → (kind, 그 id 를 달고 있는 엔티티들).
///
/// 한 엔티티가 여러 id 의 것일 수 있다 (이어져 MLINE 하나로 묶인 벽). 그래서
/// 엔티티를 id 마다 **되풀이해** 담는다 — 묶음을 쪼개면 되읽을 수가 없다.
fn by_id(drawing: &Drawing) -> BTreeMap<String, (String, Vec<&Entity>)> {
    let mut out: BTreeMap<String, (String, Vec<&Entity>)> = BTreeMap::new();
    for e in drawing.model.iter() {
        let Some(id) = identity::of(e) else { continue };
        for one in &id.ids {
            out.entry(one.clone())
                .or_insert_with(|| (id.kind.clone(), Vec::new()))
                .1
                .push(e);
        }
    }
    out
}

/// 엔진 `Drawing` 을 원본 문서에 되비친다.
///
/// `original_json` 은 **열 때 쓴 그 문서**다. `loaded` 는 그때 도면에 심긴
/// id 집합 ([`ids_in`]). 둘 다 없으면 지워진 것과 안 열린 것을 못 가린다.
pub fn drawing_to_bimove(
    original_json: &str, page_id: &str, loaded: &BTreeSet<String>, drawing: &Drawing,
) -> Result<SaveBack, String> {
    let mut doc: Value =
        serde_json::from_str(original_json).map_err(|e| format!("문서가 JSON 이 아니다: {e}"))?;
    if !doc.is_object() {
        return Err("문서가 객체가 아니다".into());
    }

    let live = by_id(drawing);
    let mut notes = Vec::new();
    let (mut kept, mut erased) = (0usize, 0usize);

    // 축척은 **면적을 되재는 데** 쓴다 (`zone_props`). 문서에 없으면
    // `1px = 1mm` 로 떨어지고 면적이 통째로 엉뚱해지므로 그걸 알려야 한다.
    let doc_scale = DimScale::from_json(doc.get("scale"));
    let scale = doc_scale.unwrap_or_default();

    let shapes = doc.get("shapes").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out: Vec<Value> = Vec::with_capacity(shapes.len());
    // 원본에 **이미 있는** id. 두 번째 저장을 위한 것이다 — 아래 설명.
    let mut in_doc: BTreeSet<&str> = BTreeSet::new();

    for sh in &shapes {
        let id = sh.get("id").and_then(Value::as_str).unwrap_or("");
        if !id.is_empty() {
            in_doc.insert(id);
        }
        // 엔진에 들어가지 않았던 도형은 **통과**다 (모듈 주석). id 가 없는
        // 것도 여기로 온다 — 우리가 만든 것이 아니다.
        if id.is_empty() || !loaded.contains(id) {
            out.push(sh.clone());
            continue;
        }
        if live.contains_key(id) {
            // 그대로. `props` 를 손대지 않는다.
            out.push(sh.clone());
            kept += 1;
        } else {
            erased += 1;
        }
    }

    // 새로 생긴 것 — 도면에만 있고 **열 때도 없었고 문서에도 없는** id.
    //
    // `loaded` 만 보면 **두 번째 저장에서 도형이 둘로 늘어난다.** 첫 저장이
    // 넣은 도형은 문서에는 있지만 `loaded`(열 때의 집합)에는 없다. 그래서
    // 위 고리에서 "통과" 로 한 번 실리고, 여기서 "새로 생김" 으로 또 실린다 —
    // 같은 id 가 두 벌이 되고, 다시 열면 공간이 겹쳐 그려진다.
    //
    // 열 때의 집합을 저장할 때마다 갱신하는 길도 있지만, 그러면 저장이
    // 엔진 상태를 바꾼다. 문서를 보는 쪽이 간단하고 저장을 몇 번 눌러도 같다.
    let mut added = 0usize;
    // 공간을 **이번에 실제로 되읽었나.** 축척 경고가 여기 매달린다.
    let mut measured_zone = false;
    let mut unread: BTreeMap<String, usize> = BTreeMap::new();
    for (id, (kind, ents)) in &live {
        if loaded.contains(id) || in_doc.contains(id.as_str()) {
            continue;
        }
        match rebuild(id, kind, ents, page_id, &scale) {
            Some(sh) => {
                out.push(sh);
                added += 1;
                if kind == identity::ZONE {
                    measured_zone = true;
                }
            }
            None => *unread.entry(kind.clone()).or_insert(0) += 1,
        }
    }
    for (kind, n) in &unread {
        notes.push(format!("새로 그린 {kind} {n}개를 저장본으로 되읽지 못했다"));
    }
    // 축척을 못 찾았는데 공간을 되읽었으면 **면적이 px 단위로 재진 것이다.**
    // 조용히 넘기면 발주 수량이 통째로 틀린 채 저장된다.
    //
    // **이번에 되읽은 공간**이 있을 때만 말한다. 도면에 공간이 떠 있다는
    // 것만 보면, 문 하나를 그려 저장해도 "면적을 쟀다" 가 떠서 재지도 않은
    // 것을 경고한다 — 늘 뜨는 경고는 읽히지 않는다.
    if doc_scale.is_none() && measured_zone {
        notes.push(
            "축척을 문서에서 못 찾아 1px = 1mm 로 면적을 쟀다 (실제와 다를 수 있다)".to_string(),
        );
    }

    if let Some(m) = doc.as_object_mut() {
        m.insert("shapes".into(), Value::Array(out));
    }

    Ok(SaveBack { json: doc.to_string(), notes, kept, erased, added })
}

/// 되읽은 도형 하나. `props` 바깥의 자리도 종류마다 달라서 같이 든다.
struct Rebuilt {
    /// 도형의 자리 (캔버스 좌표). 공간은 첫 꼭짓점, 벽·치수는 시작점,
    /// 문·창은 가운데다.
    x: f64,
    y: f64,
    rotation: f64,
    props: Value,
}

/// 엔티티들에서 Bimove 도형을 되읽는다. 못 읽으면 `None` — 부르는 쪽이 센다.
///
/// 종류를 하나씩 연다. 짐작으로 만들어 넣으면 **틀린 도형이 저장본에 남고**,
/// 그건 못 읽은 것보다 나쁘다.
fn rebuild(
    id: &str, kind: &str, ents: &[&Entity], page_id: &str, scale: &DimScale,
) -> Option<Value> {
    // **tldraw 는 도형 레코드 id 에 규칙이 있다** — `shape:` 로 시작해야 한다
    // (`@tldraw/tlschema` 의 `idValidator`). 어긴 id 를 문서에 넣으면 저장은
    // 성공한 척하고, 그 도면을 **에디터가 통째로 못 연다** ("도면을 열지
    // 못했습니다" 한 장만 뜬다).
    //
    // 한 번 그렇게 깨뜨렸다. 그리는 쪽(`cadIdentity.newId`)을 고쳤지만,
    // 여기서도 막는다 — 저장본을 못 쓰게 만드는 것보다 "되읽지 못했다" 로
    // 세는 쪽이 비교도 안 되게 낫다.
    if !id.starts_with("shape:") {
        return None;
    }
    let r = match kind {
        identity::ZONE => zone_props(ents, scale)?,
        identity::WALL => wall_props(id, ents)?,
        identity::DIM => dim_props(ents)?,
        identity::DOOR | identity::WINDOW => opening_props(kind, ents)?,
        identity::LINE => line_props(ents)?,
        // 물량표는 안 연다. 엔진 TABLE 이라 칸 내용이 다 들어 있지만, 애초에
        // CadCraft 화면에 물량표 도구가 없다 (`cadDraw.ts` 의 `CadTool`).
        // 그릴 수 없는 것을 되읽는 길부터 깔 이유가 없다.
        _ => return None,
    };
    let mut m = Map::new();
    m.insert("id".into(), json!(id));
    m.insert("type".into(), json!(type_of(kind)?));
    if !page_id.is_empty() {
        m.insert("parentId".into(), json!(page_id));
    }
    m.insert("x".into(), json!(r.x));
    m.insert("y".into(), json!(r.y));
    m.insert("rotation".into(), json!(r.rotation));
    m.insert("props".into(), r.props);
    Some(Value::Object(m))
}

/// 엔진 좌표를 캔버스 좌표로. y 만 뒤집는다 (`bimove.rs` 의 역).
fn canvas(x: f64, y: f64) -> (f64, f64) {
    (x, -y)
}

/// 소수 둘째까지. 화면의 `measurePolygon` 과 같은 자리에서 끊는다 — 다르면
/// 그려 넣은 공간과 불러온 공간의 숫자가 갈린다.
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// 글자 엔티티에서 (높이, 내용, 색). `Text` 와 `MText` 를 **둘 다** 본다.
///
/// 두 꼴이 다 나오는 게 요점이다. 저장본에서 온 공간은 `MText` 고
/// (`zone.rs` 의 `text`), 화면에서 그려 넣은 공간은 엔진 `text` 명령이라
/// `Text` 다 (`cadShapes.ts` 의 `zoneLabelCommand`). 한쪽만 보면 **방금 그린
/// 공간의 이름이 저장할 때 사라진다.**
fn text_bits(e: &Entity) -> Option<(f64, String, Color)> {
    match &e.kind {
        EntityKind::MText(t) => Some((t.height, strip_mtext(&t.contents), e.common.color)),
        EntityKind::Text(t) => Some((t.height, t.value.clone(), e.common.color)),
        _ => None,
    }
}

/// 공간 하나를 되읽는다.
///
/// 엔티티는 면(해치) + 테두리(닫힌 폴리라인) + 이름 (+ 면적) 이다. 점은
/// **테두리**에서 뜬다 — 해치 loop 에도 같은 점이 있지만 폴리라인이 양쪽
/// 경로에 다 있는 유일한 것이다 (`zone.rs`, `cadShapes.ts`).
///
/// ── 면적을 다시 잰다 ──
///
/// `zone.rs` 는 불러온 공간의 면적을 **다시 재지 않는다** (화면이 띄우는 숫자와
/// 갈리면 어느 게 맞는지 못 가린다). 여기는 사정이 다르다: 되읽는 것은 **방금
/// 그린** 공간이고 거기엔 굳은 숫자가 없다 — 화면이 면적 글자를 안 만든다.
/// 그래서 폴리곤에서 재되, 화면과 **같은 식**으로 잰다 (`areaMeasure.ts` 의
/// `pxAreaToM2`/`pxLengthToM`, 소수 둘째까지).
///
/// 면적 글자가 있으면 그걸 쓴다 — 저장본에서 온 공간을 복사한 경우다.
fn zone_props(ents: &[&Entity], scale: &DimScale) -> Option<Rebuilt> {
    let border = ents.iter().find_map(|e| match &e.kind {
        EntityKind::LwPolyline(p) if p.closed && p.vertices.len() >= 3 => Some(p),
        _ => None,
    })?;
    let pts: Vec<(f64, f64)> = border.vertices.iter().map(|v| canvas(v.p.x, v.p.y)).collect();
    let (ox, oy) = pts[0];
    // `props.points` 는 **첫 점 기준 상대 좌표**다 (`zone_of` 주석). 절대
    // 좌표로 적어도 읽히긴 하지만, 공간 도구가 만드는 꼴과 같아야 `points[0]`
    // 이 원점이라고 보는 읽는 쪽이 어긋나지 않는다.
    let points: Vec<Value> =
        pts.iter().map(|(x, y)| json!({ "x": x - ox, "y": y - oy })).collect();

    let mut texts: Vec<(f64, String, Color)> = ents.iter().filter_map(|e| text_bits(e)).collect();
    // 큰 글자가 이름이다 (`zone.rs` 의 14 > 11, 화면은 이름 하나뿐).
    texts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let (label, color) = match texts.first() {
        // 화면이 빈 이름을 "미지정" 으로 **띄운다** — 그 글자를 이름으로 박으면
        // 사용자가 적지도 않은 이름이 저장본에 남는다.
        Some((_, text, ink)) => {
            let name = if text == "미지정" { String::new() } else { text.clone() };
            (name, hex_of(*ink))
        }
        None => (String::new(), "#1a73e8".to_string()),
    };

    let area_px2 = shoelace(&pts);
    let ppm = scale.px_per_mm;
    let area = texts
        .iter()
        .skip(1)
        .find_map(|(_, t, _)| area_of(t))
        .unwrap_or_else(|| round2(area_px2 / (ppm * ppm) / 1_000_000.0));
    let perimeter = round2(perimeter(&pts) / ppm / 1000.0);

    Some(Rebuilt {
        x: ox,
        y: oy,
        rotation: 0.0,
        props: json!({
            "points": points,
            "label": label,
            // 층고는 엔티티에 안 들어 있다 — 그릴 것이 없다. 화면 기본값과 같은
            // 수를 둔다 (`ZoneShape.getDefaultProps` 의 2400).
            "wallHeightMm": 2400,
            "areaM2": area,
            "perimeterM": perimeter,
            "color": color,
        }),
    })
}

/// 신발끈 넓이. **늘 양수** — 시계 방향으로 찍으면 부호가 뒤집히고, 음수
/// 면적에 마감재를 곱하면 음수 자재가 나온다 (`cadShapes.ts` 와 같은 이유).
fn shoelace(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let (ax, ay) = pts[i];
        let (bx, by) = pts[(i + 1) % n];
        sum += ax * by - bx * ay;
    }
    sum.abs() / 2.0
}

fn perimeter(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    if n < 2 {
        return 0.0;
    }
    (0..n)
        .map(|i| {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[(i + 1) % n];
            (bx - ax).hypot(by - ay)
        })
        .sum()
}

/// 벽 하나를 되읽는다.
///
/// ── 한 엔티티가 벽 여럿이다 ──
///
/// Bimove 의 벽 도형은 **선분 하나**인데, 엔진에서는 이어진 벽이 한 엔티티로
/// 묶인다 — 저장본에서 온 것은 MLINE, 화면에서 그려 넣은 것은 **폭 있는
/// 폴리라인**이다 (`cadDraw.ts` 의 `commitCommand` 가 `pline` + `width` 를
/// 보낸다. 알려진 차이고, 모양만 닮았을 뿐 같은 엔티티가 아니다).
///
/// 그래서 꼭짓점 n+1 개가 벽 n 개가 되고, 정체성의 `ids` 가 **그 차례대로**
/// n 개여야 한다 (`wall.rs` 의 `WallChain::ids`). `id` 가 몇 번째인지로 자기
/// 구간을 집는다.
///
/// 수가 안 맞으면 `None` 이다. 첫 구간을 짐작으로 집으면 **나머지 벽이 조용히
/// 사라진다** — 세어서 알리는 쪽이 낫다.
fn wall_props(id: &str, ents: &[&Entity]) -> Option<Rebuilt> {
    for e in ents {
        let ids = identity::of(e)?.ids;
        let idx = ids.iter().position(|x| x == id)?;
        let (verts, thickness) = match &e.kind {
            EntityKind::MLine(m) => (
                m.vertices.iter().map(|v| canvas(v.p.x, v.p.y)).collect::<Vec<_>>(),
                m.scale,
            ),
            EntityKind::LwPolyline(p) if p.const_width > 0.0 => (
                p.vertices.iter().map(|v| canvas(v.p.x, v.p.y)).collect::<Vec<_>>(),
                p.const_width,
            ),
            _ => continue,
        };
        // 열린 사슬이라야 구간과 id 가 하나씩 맞물린다. 닫힌 것은 꼭짓점 수가
        // 곧 구간 수여서 여기 식이 틀린다 — 지금 그릴 길이 없으므로 안 연다.
        if verts.len() != ids.len() + 1 || idx + 1 >= verts.len() {
            continue;
        }
        let (ax, ay) = verts[idx];
        let (bx, by) = verts[idx + 1];
        return Some(Rebuilt {
            x: ax,
            y: ay,
            rotation: 0.0,
            props: json!({ "x2": bx - ax, "y2": by - ay, "thickness": thickness }),
        });
    }
    None
}

/// 문·창 하나를 되읽는다.
///
/// ── 가림막 하나에 다 들어 있다 ──
///
/// 하나가 엔티티 다섯이지만(가림 + 잼브 둘 + 문짝 + 호) 자리와 크기는
/// **WIPEOUT 네 꼭짓점**에서 다 나온다 (`opening.rs` 의 `gap_and_jambs`):
///
/// ```text
///   p0 = at(-hw,-ht)   p1 = at( hw,-ht)   p2 = at( hw, ht)   p3 = at(-hw, ht)
/// ```
///
/// 그래서 `p1-p0` 가 벽을 따라가는 축이고 그 길이가 **폭**, `p3-p0` 가 벽을
/// 가로지르는 축이고 그 길이가 **두께**, 넷의 평균이 **가운데**다. 잼브와 호를
/// 안 봐도 된다.
///
/// ── 여는 쪽은 문짝에서 ──
///
/// 문짝은 경첩에서 열린 쪽으로 제 폭만큼 뻗는 선분이다 — 제 좌표로 보면 한
/// 끝이 `y = 0` 이고 다른 끝이 `y = ±폭` 이다. 잼브는 둘 다 `y = ±두께/2` 라
/// 거기서 갈린다. 호는 **안 본다**: 거울 변환 때문에 도는 방향이
/// `flipped`·`swing` 조합마다 뒤집혀서, 되짚으면 네 경우를 따로 써야 한다
/// (`opening.rs` 의 `door` 주석). 문짝 선이 같은 것을 더 곧게 알려준다.
fn opening_props(kind: &str, ents: &[&Entity]) -> Option<Rebuilt> {
    let b = ents.iter().find_map(|e| match &e.kind {
        EntityKind::Wipeout(w) if w.boundary.len() == 4 => Some(&w.boundary),
        _ => None,
    })?;
    let (p0, p1, p3) = (b[0], b[1], b[3]);

    let along = p1 - p0;
    let across = p3 - p0;
    let width = along.dist(Vec2::ZERO);
    let thickness = across.dist(Vec2::ZERO);
    if !(width > 0.0) || !(thickness > 0.0) {
        return None;
    }
    let u = along / width;
    let v = across / thickness;
    let c = (b[0] + b[1] + b[2] + b[3]) / 4.0;

    // `frame` 의 역: u = (cos r, -sin r) 이므로 r = atan2(-u.y, u.x).
    let rotation = (-u.y).atan2(u.x);
    let (cx, cy) = canvas(c.x, c.y);

    let mut props = Map::new();
    props.insert("width".into(), json!(width));
    props.insert("thickness".into(), json!(thickness));

    if kind == identity::DOOR {
        // 제 좌표로 옮겨 문짝을 가린다 — 한 끝이 `y = 0` 인 선분이다.
        let local = |p: Vec2| {
            let d = p - c;
            (d.x * u.x + d.y * u.y, d.x * v.x + d.y * v.y)
        };
        let leaf = ents.iter().find_map(|e| match &e.kind {
            EntityKind::Line(l) => {
                let (a, bb) = (local(Vec2::new(l.a.x, l.a.y)), local(Vec2::new(l.b.x, l.b.y)));
                // 문짝: 한 끝이 가운데 선 위(y≈0), 다른 끝이 폭만큼 떨어져 있다.
                let eps = thickness.max(1e-6) * 0.25;
                if a.1.abs() <= eps && bb.1.abs() > eps {
                    Some((a, bb))
                } else if bb.1.abs() <= eps && a.1.abs() > eps {
                    Some((bb, a))
                } else {
                    None
                }
            }
            _ => None,
        });
        // 문짝을 못 찾으면 화면 기본값으로 둔다 — 문은 살리는 게 낫다.
        // `swing` 이 0 이면 문짝이 벽 위에 겹쳐 눕는다 (`opening_of` 주석).
        let (side, flipped) = match leaf {
            Some((hinge, tip)) => (
                if tip.1 < 0.0 { -1.0 } else { 1.0 },
                // 경첩은 `-hw * f` 에 있다 — 오른쪽이면 뒤집힌 것이다.
                hinge.0 > 0.0,
            ),
            None => (1.0, false),
        };
        props.insert("swing".into(), json!(side));
        props.insert("flipped".into(), json!(flipped));
    }

    Some(Rebuilt { x: cx, y: cy, rotation, props: Value::Object(props) })
}

/// 그냥 선 묶음을 **`dxfgroup` 한 덩이로** 되읽는다.
///
/// ── 왜 `dxfgroup` 인가 ──
///
/// 선·폴리라인·사각형·원형은 Bimove 객체가 아니다 (`identity::LINE` 주석).
/// 그래서 전용 타입을 새로 낼 이유가 없고, 들여온 도면이 이미 가는 자리가
/// 있다 — `pathData` 한 줄에 선분을 이어붙인 `dxfgroup` 이다. 엔진은 그걸
/// 읽는 길을 이미 들고 있으므로 (`bimove.rs` 의 `parse_path`) 다시 열면
/// 같은 선이 그대로 선다.
///
/// ── 원·호는 선분으로 쪼갠다 ──
///
/// `pathData` 는 `M`·`L`·`Z` 뿐이다 (`parse_path`). `A` 를 써 봐야 읽는 쪽이
/// "모르는 명령" 으로 세고 그 선을 버린다 — 저장은 됐다고 하고 다시 열면
/// 원이 없는 꼴이다. 들여올 때 호·원·스플라인을 이미 선분으로 쪼개는 것과
/// 같은 결정이다. 반지름을 고치는 길은 이 쪼갬과 함께 사라진다 (지금 그
/// 길이 없고, 생기면 전용 타입이 필요하다 — 여기 적어 둔다).
///
/// ── 하나라도 못 읽으면 덩이를 안 만든다 ──
///
/// 반만 담으면 다시 열 때 **모양이 달라진 채로** 선다. 그건 안 저장된 것보다
/// 나쁘다 — 부르는 쪽이 `None` 을 "되읽지 못했다" 로 세어 사람에게 띄운다.
fn line_props(ents: &[&Entity]) -> Option<Rebuilt> {
    let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
    // 선 굵기. 폴리라인 폭이 있으면 그걸 쓴다 (벽이 아닌 굵은 선).
    let mut width = 0.0f64;
    for e in ents {
        match &e.kind {
            EntityKind::Line(l) => {
                runs.push(vec![canvas(l.a.x, l.a.y), canvas(l.b.x, l.b.y)]);
            }
            EntityKind::LwPolyline(p) if p.vertices.len() >= 2 => {
                let mut run: Vec<(f64, f64)> =
                    p.vertices.iter().map(|v| canvas(v.p.x, v.p.y)).collect();
                // 닫힌 것은 **첫 점으로 돌아오게** 적는다. `Z` 로 적어도
                // 읽히지만, 들여온 도면이 `M…L…` 만 쓰므로 꼴을 맞춘다.
                if p.closed {
                    run.push(run[0]);
                }
                runs.push(run);
                width = width.max(p.const_width);
            }
            EntityKind::Circle(c) => {
                runs.push(arc_run(c.center.x, c.center.y, c.radius, 0.0, std::f64::consts::TAU));
            }
            EntityKind::Arc(a) => {
                runs.push(arc_run(a.center.x, a.center.y, a.radius, a.start, a.end));
            }
            // 모르는 엔티티. 위 주석 — 반만 담지 않는다.
            _ => return None,
        }
    }
    if runs.is_empty() {
        return None;
    }

    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for r in &runs {
        for &(x, y) in r {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    // 점은 **도형 자리 기준**으로 적는다 — 엔진이 읽을 때 `x,y` 를 더한다
    // (`bimove.rs` 의 `at`). 절대 좌표로 적어도 엔진은 같이 읽지만, tldraw
    // 쪽 `w`·`h` 가 도형 자리에서 재는 상자라서 선택이 엉뚱한 데 잡힌다.
    let mut d = String::new();
    let mut segs = 0usize;
    for r in &runs {
        for (i, &(x, y)) in r.iter().enumerate() {
            d.push(if i == 0 { 'M' } else { 'L' });
            d.push_str(&format!("{},{}", round2(x - x0), round2(y - y0)));
            if i > 0 {
                segs += 1;
            }
        }
    }

    Some(Rebuilt {
        x: x0,
        y: y0,
        rotation: 0.0,
        props: json!({
            "w": round2(x1 - x0),
            "h": round2(y1 - y0),
            "pathData": d,
            "thickness": if width > 0.0 { round2(width) } else { 1.0 },
            "segCount": segs,
            "textsJson": "",
            "hatchesJson": "",
        }),
    })
}

/// 원·호를 선분 줄로 쪼갠다. 엔진 좌표를 받아 **캔버스 좌표**를 돌려준다.
///
/// 쪼갬 수는 한 바퀴에 64 다 — 화면의 미리보기가 48 각형인데
/// (`cadDraw.ts` 의 `PREVIEW_ARC_STEPS`), 저장본에 굳는 쪽은 한 번 적으면
/// 못 되돌리므로 더 촘촘하게 둔다. 지름 10m 짜리 원에서 꼭짓점 하나 사이가
/// 0.5mm 쯤이라 눈에 안 띈다.
fn arc_run(cx: f64, cy: f64, r: f64, start: f64, end: f64) -> Vec<(f64, f64)> {
    const FULL_STEPS: f64 = 64.0;
    let sweep = end - start;
    let n = ((sweep.abs() / std::f64::consts::TAU) * FULL_STEPS).ceil().max(8.0) as usize;
    (0..=n)
        .map(|i| {
            let t = start + sweep * (i as f64) / (n as f64);
            canvas(cx + r * t.cos(), cy + r * t.sin())
        })
        .collect()
}

/// 치수 하나를 되읽는다.
///
/// `p13`/`p14` 가 잰 두 점이고 `defpt` 는 **치수선 위의 점**이다 (`dim.rs`).
/// `offset` 은 그 둘 사이의 거리를 캔버스 왼쪽 법선에 투영해 뜬다 — 부호까지
/// 살려야 치수선이 반대쪽으로 넘어가지 않는다.
fn dim_props(ents: &[&Entity]) -> Option<Rebuilt> {
    let d = ents.iter().find_map(|e| match &e.kind {
        EntityKind::Dimension(d) => Some(d),
        _ => None,
    })?;
    let (ax, ay) = canvas(d.p13.x, d.p13.y);
    let (bx, by) = canvas(d.p14.x, d.p14.y);
    let (dx, dy) = (bx - ax, by - ay);
    let len = dx.hypot(dy);
    if !(len > 1e-9) {
        return None;
    }
    // 화면의 `nx = -y2/len, ny = x2/len`
    let (nx, ny) = (-dy / len, dx / len);
    let (px, py) = canvas(d.defpt.x, d.defpt.y);
    let offset = (px - ax) * nx + (py - ay) * ny;

    Some(Rebuilt {
        x: ax,
        y: ay,
        rotation: 0.0,
        props: json!({ "x2": dx, "y2": dy, "offset": offset }),
    })
}

/// MTEXT 꾸밈을 벗긴다 (`bimove.rs` 의 `to_mtext` 의 역).
fn strip_mtext(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            // `\P` 는 줄바꿈, `\\`·`\{`·`\}` 는 그 글자 자체.
            Some('P') => out.push('\n'),
            Some(e @ ('\\' | '{' | '}')) => out.push(e),
            // 나머지 제어열은 `;` 까지 버린다 (`\W0.8;`, `\H1.2x;`).
            Some(_) => {
                for c in it.by_ref() {
                    if c == ';' {
                        break;
                    }
                }
            }
            None => break,
        }
    }
    out.trim_matches(|c| c == '{' || c == '}').trim().to_string()
}

/// `12.3m²` 에서 숫자만. 꼴이 다르면 `None`.
fn area_of(s: &str) -> Option<f64> {
    s.trim().strip_suffix("m²")?.trim().parse().ok()
}

/// `Color::True` 를 `#rrggbb` 로. 다른 색 꼴이면 화면 기본값으로 떨어진다.
fn hex_of(c: Color) -> String {
    match c {
        Color::True(rgb) => format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2),
        _ => "#1a73e8".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zone::Zone;
    use cadcraft_color::Rgb;
    use cadcraft_geom::Vec2;

    /// 캔버스 좌표로 2000×1000 px 사각형 공간 하나.
    ///
    /// 1px = 1mm 로 보면 2m × 1m = **2m²** 다. 되읽기가 면적을 다시 재므로
    /// 눈으로 셈이 되는 크기를 쓴다.
    fn zone(id: &str, label: &str, area: f64) -> Zone {
        Zone {
            id: id.to_string(),
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(2000.0, 0.0),
                Vec2::new(2000.0, 1000.0),
                Vec2::new(0.0, 1000.0),
            ],
            label: label.to_string(),
            area_m2: area,
            color: Rgb(0x34, 0xa8, 0x53),
            layer: "0".into(),
        }
    }

    /// 축척이 든 빈 문서. 없으면 되읽기가 `1px = 1mm` 로 떨어지며 `notes` 를 올린다.
    fn empty_doc() -> String {
        json!({
            "pages": [{ "id": "page:1", "name": "1" }],
            "scale": { "pxPerMm": 1.0 },
            "shapes": [],
        })
        .to_string()
    }

    /// 공간들을 넣은 도면.
    fn drawn(zones: &[Zone]) -> Drawing {
        let mut d = Drawing::new_metric();
        crate::zone::add_zones(&mut d, zones);
        d
    }

    fn loaded(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn shapes_of(json: &str) -> Vec<Value> {
        serde_json::from_str::<Value>(json)
            .unwrap()
            .get("shapes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap()
    }

    fn ids_of(json: &str) -> Vec<String> {
        shapes_of(json)
            .iter()
            .map(|s| s.get("id").and_then(Value::as_str).unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn 열지_않은_도형은_지우지_않는다() {
        // **이 파일에서 제일 중요한 성질이다.** 정방향이 조용히 빼는 자리가
        // 여럿이라 (돌아간 그룹, 두께 0 인 벽, 다른 페이지) 그 도형들은 도면에
        // 정체성이 없다. `loaded` 를 안 보고 비교하면 저장할 때마다 사라진다.
        let doc = json!({
            "pages": [{ "id": "page:1", "name": "1" }],
            "shapes": [
                { "id": "shape:rotated", "type": "wall", "parentId": "page:1", "props": {} },
                { "id": "shape:other", "type": "zone", "parentId": "page:2", "props": {} },
            ],
        })
        .to_string();

        // 아무것도 안 열렸다 — 심긴 id 가 없다.
        let back = drawing_to_bimove(&doc, "page:1", &BTreeSet::new(), &Drawing::new_metric())
            .expect("되돌리기 실패");

        assert_eq!(back.erased, 0, "열지도 않은 도형을 지웠다");
        assert_eq!(ids_of(&back.json), vec!["shape:rotated", "shape:other"]);
    }

    #[test]
    fn 지운_것은_뺀다() {
        let doc = json!({
            "pages": [{ "id": "page:1", "name": "1" }],
            "shapes": [
                { "id": "shape:a", "type": "zone", "parentId": "page:1", "props": { "label": "방" } },
                { "id": "shape:b", "type": "zone", "parentId": "page:1", "props": { "label": "거실" } },
            ],
        })
        .to_string();

        // 둘 다 열렸는데 도면에는 a 만 남아 있다 — b 를 지운 것이다.
        let d = drawn(&[zone("shape:a", "방", 5.0)]);
        let back = drawing_to_bimove(&doc, "page:1", &loaded(&["shape:a", "shape:b"]), &d)
            .expect("되돌리기 실패");

        assert_eq!(back.kept, 1);
        assert_eq!(back.erased, 1);
        assert_eq!(ids_of(&back.json), vec!["shape:a"]);
    }

    #[test]
    fn 그대로인_것은_props_를_안_고친다() {
        // 1차의 범위다 — 엔진에서 옮겨도 원본이 정본이다. 되읽기가 손실
        // 없이 되는지 확인되기 전에 덮어쓰면 그 손실이 저장본에 남는다.
        let props = json!({ "label": "방", "wallHeightMm": 2700, "areaM2": 5.0, "남의것": [1, 2] });
        let doc = json!({
            "pages": [{ "id": "page:1", "name": "1" }],
            "shapes": [
                { "id": "shape:a", "type": "zone", "parentId": "page:1", "props": props },
            ],
        })
        .to_string();

        // 이름·면적을 **다르게** 그려 넣었는데도 원본이 남아야 한다.
        let d = drawn(&[zone("shape:a", "딴이름", 99.0)]);
        let back = drawing_to_bimove(&doc, "page:1", &loaded(&["shape:a"]), &d)
            .expect("되돌리기 실패");

        assert_eq!(shapes_of(&back.json)[0].get("props").unwrap(), &props);
    }

    #[test]
    fn 엔진이_모르는_것은_통과한다() {
        // 밑그림 image 와 그 asset, tldraw 도형, 문서 바깥의 키까지.
        let doc = json!({
            "schemaVersion": 3,
            "scale": { "pxPerMm": 0.5 },
            "assets": [{ "id": "asset:1", "type": "image" }],
            "pages": [{ "id": "page:1", "name": "1" }],
            "shapes": [
                { "id": "shape:img", "type": "image", "parentId": "page:1", "props": { "assetId": "asset:1" } },
                { "id": "shape:geo", "type": "geo", "parentId": "page:1", "props": {} },
            ],
        })
        .to_string();

        let back = drawing_to_bimove(&doc, "page:1", &BTreeSet::new(), &Drawing::new_metric())
            .expect("되돌리기 실패");

        let got: Value = serde_json::from_str(&back.json).unwrap();
        let want: Value = serde_json::from_str(&doc).unwrap();
        assert_eq!(got, want, "원본이 그대로 돌아오지 않았다");
    }

    #[test]
    fn 새로_그린_공간을_되읽는다() {
        // 열 때는 없었던 id — 화면에서 그려 넣은 것이다 (`cadIdentity.newId`).
        let d = drawn(&[zone("shape:bimove:zone:xyz", "주방", 5.0)]);
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");

        assert_eq!(back.added, 1);
        assert!(back.notes.is_empty(), "되읽었는데 못 읽었다고 적혔다: {:?}", back.notes);

        let sh = &shapes_of(&back.json)[0];
        assert_eq!(sh.get("id").unwrap(), "shape:bimove:zone:xyz");
        assert_eq!(sh.get("type").unwrap(), "zone");
        assert_eq!(sh.get("parentId").unwrap(), "page:1");
        // 자리는 첫 꼭짓점이다 — `props.points` 가 거기 기준이라서 (`zone_of`).
        assert_eq!(sh.get("x").unwrap(), 0.0);
        assert_eq!(sh.get("y").unwrap(), 0.0);

        let p = sh.get("props").unwrap();
        assert_eq!(p.get("label").unwrap(), "주방");
        // 면적 글자가 있으면 그걸 쓴다 (다시 재면 2.0 이 나오는 자리다).
        assert_eq!(p.get("areaM2").unwrap(), 5.0);
        // 글자 색이 공간 색이다 (면이 아니다 — `zoneStyle.ts`).
        assert_eq!(p.get("color").unwrap(), "#34a853");
        // 캔버스는 y 가 아래로 간다 — 엔진에서 뒤집힌 것이 되돌아와야 한다.
        assert_eq!(
            p.get("points").unwrap(),
            &json!([
                { "x": 0.0, "y": 0.0 },
                { "x": 2000.0, "y": 0.0 },
                { "x": 2000.0, "y": 1000.0 },
                { "x": 0.0, "y": 1000.0 },
            ]),
        );
    }

    #[test]
    fn tldraw_가_못_읽는_id_는_안_넣는다() {
        // tldraw 는 도형 레코드 id 가 `shape:` 로 시작해야 한다. 어긴 id 를
        // 문서에 넣으면 저장은 성공한 척하고 **에디터가 그 도면을 통째로 못
        // 연다** — 실제로 한 번 그렇게 깨뜨렸다.
        let d = drawn(&[zone("bimove:zone:xyz", "주방", 5.0)]);
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");

        assert_eq!(back.added, 0, "규칙을 어긴 id 를 넣었다");
        assert!(shapes_of(&back.json).is_empty());
        assert_eq!(back.notes.len(), 1, "조용히 뺐다: {:?}", back.notes);
    }

    #[test]
    fn 두_번_저장해도_도형이_안_늘어난다() {
        // 첫 저장이 넣은 도형은 문서에는 있지만 `loaded`(열 때의 집합)에는
        // 없다. `loaded` 만 보면 "통과" 로 한 번 실리고 "새로 생김" 으로 또
        // 실려서 **같은 id 가 두 벌**이 된다 — 다시 열면 공간이 겹쳐 그려진다.
        let d = drawn(&[zone("shape:bimove:zone:xyz", "주방", 5.0)]);

        let first = drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d)
            .expect("첫 저장 실패");
        assert_eq!(first.added, 1);
        assert_eq!(ids_of(&first.json), vec!["shape:bimove:zone:xyz"]);

        // 같은 도면을 **다시** 저장한다. 엔진은 그대로고 문서만 갱신됐다.
        let second = drawing_to_bimove(&first.json, "page:1", &BTreeSet::new(), &d)
            .expect("두 번째 저장 실패");
        assert_eq!(second.added, 0, "같은 도형을 또 넣었다");
        assert_eq!(ids_of(&second.json), vec!["shape:bimove:zone:xyz"]);
        assert!(second.notes.is_empty(), "notes: {:?}", second.notes);
    }

    #[test]
    fn 면적_글자가_없으면_폴리곤에서_다시_잰다() {
        // 화면에서 그려 넣은 공간은 면적 글자를 안 만든다 (`cadShapes.ts` 는
        // 이름표 하나뿐이다). 다시 재지 않으면 발주 수량이 0 으로 저장된다.
        let d = drawn(&[zone("shape:bimove:zone:xyz", "주방", 0.0)]);
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");

        let p = shapes_of(&back.json)[0].get("props").unwrap().clone();
        // 2000 × 1000 px, 1px = 1mm → 2m × 1m
        assert_eq!(p.get("areaM2").unwrap(), 2.0);
        assert_eq!(p.get("perimeterM").unwrap(), 6.0);
    }

    #[test]
    fn 공간을_안_되읽었으면_축척_얘기를_안_한다() {
        // 도면에 공간이 떠 있다는 것만 보고 말하면, 문 하나를 그려 저장해도
        // "면적을 쟀다" 가 뜬다 — 재지도 않은 것을 경고하는 셈이고, 늘 뜨는
        // 경고는 읽히지 않는다.
        use crate::opening::{OpeningKind, Swing};
        let mut d = Drawing::new_metric();
        // 공간은 **열 때부터 있던 것**으로 둔다 (되읽지 않는다).
        crate::zone::add_zones(&mut d, &[zone("shape:old:zone", "방", 5.0)]);
        let loaded_ids = ids_in(&d);
        crate::opening::add_openings(&mut d, &[opening(
            OpeningKind::Door(Swing { side: 1.0, flipped: false }),
            0.0,
        )]);

        // 축척이 없는 문서다.
        let doc = json!({
            "pages": [{ "id": "page:1", "name": "1" }],
            "shapes": [{ "id": "shape:old:zone", "type": "zone", "parentId": "page:1", "props": {} }],
        })
        .to_string();

        let back = drawing_to_bimove(&doc, "page:1", &loaded_ids, &d).expect("되돌리기 실패");
        assert_eq!(back.added, 1, "문을 못 읽었다: {:?}", back.notes);
        assert!(
            !back.notes.iter().any(|n| n.contains("축척")),
            "공간을 안 되읽었는데 축척 얘기를 했다: {:?}",
            back.notes,
        );
    }

    #[test]
    fn 축척이_없으면_면적을_쟀다고_알린다() {
        // 조용히 1px = 1mm 로 재면 발주 수량이 통째로 틀린 채 저장된다.
        let doc = json!({ "pages": [{ "id": "page:1", "name": "1" }], "shapes": [] }).to_string();
        let d = drawn(&[zone("shape:bimove:zone:xyz", "주방", 0.0)]);

        let back = drawing_to_bimove(&doc, "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 1);
        assert_eq!(back.notes.len(), 1);
        assert!(back.notes[0].contains("축척"), "축척 얘기가 없다: {:?}", back.notes);
    }

    #[test]
    fn 미지정은_빈_이름으로_되읽는다() {
        // 화면이 빈 이름을 "미지정" 으로 **띄운다.** 그 글자를 이름으로 박으면
        // 사용자가 적지도 않은 이름이 저장본에 남는다.
        let d = drawn(&[zone("shape:bimove:zone:xyz", "", 0.0)]);
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");

        let p = shapes_of(&back.json)[0].get("props").unwrap().clone();
        assert_eq!(p.get("label").unwrap(), "");
    }

    #[test]
    fn 그려_넣은_이름은_text_엔티티다() {
        // 저장본에서 온 공간은 `MText` 고 화면에서 그려 넣은 것은 엔진 `text`
        // 명령이라 `Text` 다 (`cadShapes.ts` 의 `zoneLabelCommand`). 한쪽만
        // 보면 **방금 그린 공간의 이름이 저장할 때 사라진다.**
        let mut d = Drawing::new_metric();
        let from = d.model.len();
        let h = d.new_handle();
        d.model.push(Entity::new(
            h,
            EntityKind::LwPolyline(cadcraft_doc::LwPolyline {
                vertices: vec![
                    cadcraft_geom::PolyVertex::new(Vec2::new(0.0, 0.0)),
                    cadcraft_geom::PolyVertex::new(Vec2::new(2000.0, 0.0)),
                    cadcraft_geom::PolyVertex::new(Vec2::new(2000.0, -1000.0)),
                    cadcraft_geom::PolyVertex::new(Vec2::new(0.0, -1000.0)),
                ],
                closed: true,
                const_width: 0.0,
                elevation: 0.0,
                plinegen: false,
            }),
        ));
        let h = d.new_handle();
        d.model.push(Entity::new(
            h,
            EntityKind::Text(cadcraft_doc::Text {
                insert: cadcraft_geom::Vec3::new(1000.0, -500.0, 0.0),
                align_pt: None,
                // 화면은 공간 크기에 맞춰 높이를 정한다 — 고정값이 아니다.
                height: 5.37,
                value: "주방".into(),
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                style: "Standard".into(),
                halign: Default::default(),
                valign: Default::default(),
            }),
        ));
        crate::identity::stamp_one(&mut d, from, crate::identity::ZONE, "shape:bimove:zone:xyz");

        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 1, "notes: {:?}", back.notes);
        let p = shapes_of(&back.json)[0].get("props").unwrap().clone();
        assert_eq!(p.get("label").unwrap(), "주방");
        assert_eq!(p.get("areaM2").unwrap(), 2.0);
    }

    #[test]
    fn 벽_사슬을_구간마다_되읽는다() {
        // Bimove 의 벽 도형은 **선분 하나**인데 엔진에서는 이어진 벽이 한
        // 엔티티다. 꼭짓점 셋 = 구간 둘 = 벽 도형 둘.
        let mut d = Drawing::new_metric();
        let from = d.model.len();
        let h = d.new_handle();
        d.model.push(Entity::new(
            h,
            EntityKind::LwPolyline(cadcraft_doc::LwPolyline {
                vertices: vec![
                    cadcraft_geom::PolyVertex::new(Vec2::new(0.0, 0.0)),
                    cadcraft_geom::PolyVertex::new(Vec2::new(1000.0, 0.0)),
                    cadcraft_geom::PolyVertex::new(Vec2::new(1000.0, -500.0)),
                ],
                closed: false,
                // 그려 넣은 벽은 **폭 있는 폴리라인**이다 (`commitCommand`).
                const_width: 150.0,
                elevation: 0.0,
                plinegen: false,
            }),
        ));
        let ids = ["shape:bimove:wall:a".to_string(), "shape:bimove:wall:b".to_string()];
        crate::identity::stamp(&mut d, from, crate::identity::WALL, &ids);

        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 2, "notes: {:?}", back.notes);

        let shapes = shapes_of(&back.json);
        let a = shapes.iter().find(|s| s.get("id").unwrap() == "shape:bimove:wall:a").unwrap();
        assert_eq!(a.get("x").unwrap(), 0.0);
        assert_eq!(a.get("y").unwrap(), 0.0);
        assert_eq!(a.get("props").unwrap().get("x2").unwrap(), 1000.0);
        assert_eq!(a.get("props").unwrap().get("y2").unwrap(), 0.0);
        assert_eq!(a.get("props").unwrap().get("thickness").unwrap(), 150.0);

        let b = shapes.iter().find(|s| s.get("id").unwrap() == "shape:bimove:wall:b").unwrap();
        // 캔버스는 y 가 아래로 간다 — 엔진 -500 이 캔버스 +500 이다.
        assert_eq!(b.get("x").unwrap(), 1000.0);
        assert_eq!(b.get("y").unwrap(), 0.0);
        assert_eq!(b.get("props").unwrap().get("y2").unwrap(), 500.0);
    }

    #[test]
    fn 벽_id_수가_구간과_안_맞으면_안_읽는다() {
        // 첫 구간을 짐작으로 집으면 **나머지 벽이 조용히 사라진다.**
        let mut d = Drawing::new_metric();
        let from = d.model.len();
        let h = d.new_handle();
        d.model.push(Entity::new(
            h,
            EntityKind::LwPolyline(cadcraft_doc::LwPolyline {
                vertices: vec![
                    cadcraft_geom::PolyVertex::new(Vec2::new(0.0, 0.0)),
                    cadcraft_geom::PolyVertex::new(Vec2::new(1000.0, 0.0)),
                    cadcraft_geom::PolyVertex::new(Vec2::new(1000.0, -500.0)),
                ],
                closed: false,
                const_width: 150.0,
                elevation: 0.0,
                plinegen: false,
            }),
        ));
        // 구간은 둘인데 id 는 하나다.
        crate::identity::stamp_one(&mut d, from, crate::identity::WALL, "shape:bimove:wall:only");

        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 0);
        assert!(back.notes.iter().any(|n| n.contains("wall")), "notes: {:?}", back.notes);
    }

    /// 손잡이 하나짜리 엔티티를 넣고 `line` 정체성을 찍는다.
    fn drawn_line(kind: EntityKind, id: &str) -> Drawing {
        let mut d = Drawing::new_metric();
        let from = d.model.len();
        let h = d.new_handle();
        d.model.push(Entity::new(h, kind));
        crate::identity::stamp_one(&mut d, from, crate::identity::LINE, id);
        d
    }

    fn pline(verts: &[(f64, f64)], closed: bool) -> EntityKind {
        EntityKind::LwPolyline(cadcraft_doc::LwPolyline {
            vertices: verts
                .iter()
                .map(|&(x, y)| cadcraft_geom::PolyVertex::new(Vec2::new(x, y)))
                .collect(),
            closed,
            const_width: 0.0,
            elevation: 0.0,
            plinegen: false,
        })
    }

    #[test]
    fn 사각형을_dxfgroup_으로_되읽는다() {
        // **엔진이 유일한 화면이 된 뒤의 핵심 성질이다.** 전엔 `line` 종류가
        // 없어서 사각형을 그려 저장하면 `to_bimove` 가 못 보고 조용히 사라졌다
        // — 그려지지 않는 도구와 같다 (하네스가 "저장본 도형 0개" 로 잡았다).
        let d = drawn_line(
            pline(&[(0.0, 0.0), (1000.0, 0.0), (1000.0, -500.0), (0.0, -500.0)], true),
            "shape:bimove:line:r",
        );
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 1, "notes: {:?}", back.notes);

        let sh = &shapes_of(&back.json)[0];
        assert_eq!(sh.get("type").unwrap(), "dxfgroup");
        assert_eq!(sh.get("parentId").unwrap(), "page:1");
        // 캔버스는 y 가 아래로 간다 — 엔진 -500 이 캔버스 +500 이고,
        // 상자의 왼쪽 위가 (0, 0) 이다.
        assert_eq!(sh.get("x").unwrap(), 0.0);
        assert_eq!(sh.get("y").unwrap(), 0.0);
        let props = sh.get("props").unwrap();
        assert_eq!(props.get("w").unwrap(), 1000.0);
        assert_eq!(props.get("h").unwrap(), 500.0);
        // 닫힌 것은 첫 점으로 돌아온다 — 꼭짓점 넷 + 되돌아오기 = 선분 넷.
        assert_eq!(props.get("segCount").unwrap(), 4);
        assert_eq!(
            props.get("pathData").unwrap().as_str().unwrap(),
            "M0,0L1000,0L1000,500L0,500L0,0"
        );
    }

    #[test]
    fn 그린_선이_저장본을_건너_같은_자리에_선다() {
        // 되읽기만 되고 **다시 읽히지 않으면** 아무 의미가 없다. 저장본을
        // 건너 엔진으로 돌아오는 자리까지 한 번에 잰다.
        let d = drawn_line(
            pline(&[(100.0, -200.0), (400.0, -200.0), (400.0, -600.0)], false),
            "shape:bimove:line:p",
        );
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        let again = crate::bimove::drawing_from_bimove(&back.json, "page:1").expect("다시 읽기 실패");
        let verts: Vec<Vec2> = again
            .drawing
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::LwPolyline(p) => Some(p.vertices.iter().map(|v| v.p).collect()),
                _ => None,
            })
            .expect("폴리라인이 없다");
        assert_eq!(
            verts,
            vec![Vec2::new(100.0, -200.0), Vec2::new(400.0, -200.0), Vec2::new(400.0, -600.0)],
            "그린 자리로 돌아와야 한다"
        );
    }

    #[test]
    fn 그린_선의_정체성이_저장본을_건너_살아남는다() {
        // **왕복에서 제일 중요한 성질이다.** 선이 같은 자리에 서는 것만으로는
        // 모자란다 — 다시 읽은 선에 정체성이 없으면 고를 수는 있어도
        // ("Polyline 3개") 패널이 "어느 Bimove 도형인가" 를 몰라서 못 본다.
        //
        // 정렬을 한 번 하면 선택이 **영영 안 돌아오던** 자리가 거기였다:
        // 고치기는 저장본을 쓰고 엔진을 다시 읽는 길이라, 다시 읽은 순간
        // 정체성이 사라져 다음 단추를 누를 수가 없었다
        // (하네스 `.browser/enginealign.mjs` 가 잡았다).
        let id = "shape:bimove:line:p";
        let d = drawn_line(pline(&[(0.0, 0.0), (500.0, 0.0), (500.0, -300.0)], false), id);
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        let again = crate::bimove::drawing_from_bimove(&back.json, "page:1").expect("다시 읽기 실패");

        assert_eq!(
            ids_in(&again.drawing),
            [id.to_string()].into_iter().collect::<BTreeSet<_>>(),
            "다시 읽은 선에 같은 id 가 박혀 있어야 한다"
        );
        // 두 번째 저장에서도 **도형이 안 늘어난다** — 정체성이 살아 있으면
        // `in_doc` 이 걸러 준다.
        let twice = drawing_to_bimove(&back.json, "page:1", &ids_in(&again.drawing), &again.drawing)
            .expect("두 번째 되돌리기 실패");
        assert_eq!(shapes_of(&twice.json).len(), 1, "같은 선이 두 벌이 되면 안 된다");
        assert_eq!(twice.added, 0);
    }

    #[test]
    fn 들여온_도면에는_정체성을_안_심는다() {
        // 심으면 선 하나를 찍어도 **수백 선짜리 덩이가 통째로** 잡힌다 —
        // 지우기 한 번에 도면이 날아가는 자리다. 들여온 것은 id 가
        // `shape:bimove:line:` 꼴이 아니라서 갈린다.
        let doc = json!({
            "pages": [{ "id": "page:1", "name": "1" }],
            "scale": { "pxPerMm": 1.0 },
            "shapes": [{
                "id": "shape:imported", "type": "dxfgroup",
                "x": 0, "y": 0, "rotation": 0, "index": "a1", "parentId": "page:1",
                "props": { "pathData": "M0,0L100,0M200,0L300,0" },
            }],
        })
        .to_string();
        let load = crate::bimove::drawing_from_bimove(&doc, "page:1").expect("읽기 실패");
        assert!(ids_in(&load.drawing).is_empty(), "들여온 선에는 정체성이 없어야 한다");
    }

    #[test]
    fn 원을_선분으로_쪼갠다() {
        // `pathData` 는 `M`·`L`·`Z` 뿐이다. `A` 로 적으면 읽는 쪽이 "모르는
        // 명령" 으로 세고 그 선을 버린다 — 저장은 됐다 하고 원이 없는 꼴이다.
        let d = drawn_line(
            EntityKind::Circle(cadcraft_doc::Circle {
                center: cadcraft_geom::Vec3::new(500.0, -500.0, 0.0),
                radius: 250.0,
            }),
            "shape:bimove:line:c",
        );
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 1, "notes: {:?}", back.notes);
        let props = shapes_of(&back.json)[0].get("props").unwrap().clone();
        let path = props.get("pathData").unwrap().as_str().unwrap().to_string();
        assert!(!path.contains('A'), "호 명령을 쓰면 안 된다: {path}");
        assert_eq!(props.get("segCount").unwrap(), 64);
        // 지름 500 짜리 원이니 상자가 500×500 이다 (쪼갬 오차는 round2 안).
        assert!((props.get("w").unwrap().as_f64().unwrap() - 500.0).abs() < 1.0);
        assert!((props.get("h").unwrap().as_f64().unwrap() - 500.0).abs() < 1.0);
    }

    #[test]
    fn 모르는_엔티티는_반만_담지_않는다() {
        // 반만 담으면 다시 열 때 **모양이 달라진 채로** 선다. 그건 안
        // 저장된 것보다 나쁘다 — 못 읽었다고 세어 사람에게 띄운다.
        let d = drawn_line(
            EntityKind::Point(cadcraft_doc::Point {
                p: cadcraft_geom::Vec3::new(0.0, 0.0, 0.0),
                angle: 0.0,
            }),
            "shape:bimove:line:x",
        );
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 0);
        assert!(back.notes.iter().any(|n| n.contains("line")), "notes: {:?}", back.notes);
    }

    #[test]
    fn 치수를_되읽는다() {
        use crate::dim::DimSeg;
        let mut d = Drawing::new_metric();
        let seg = DimSeg {
            id: "shape:bimove:dim:a".into(),
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(1000.0, 0.0),
            offset: 200.0,
            layer: "0".into(),
        };
        crate::dim::add_dims(&mut d, &[seg], &DimScale::default());

        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 1, "notes: {:?}", back.notes);

        let sh = &shapes_of(&back.json)[0];
        assert_eq!(sh.get("type").unwrap(), "dimension");
        assert_eq!(sh.get("x").unwrap(), 0.0);
        assert_eq!(sh.get("y").unwrap(), 0.0);
        let p = sh.get("props").unwrap();
        assert_eq!(p.get("x2").unwrap(), 1000.0);
        assert_eq!(p.get("y2").unwrap(), 0.0);
        // 부호까지 살아야 치수선이 반대쪽으로 넘어가지 않는다.
        assert_eq!(p.get("offset").unwrap(), 200.0);
    }

    /// 문·창을 엔진에 넣었다가 **되읽어서** 원래 값이 나오는지 본다.
    ///
    /// 되읽기를 손으로 짠 식으로만 재면 그 식이 틀려도 통과한다. 정방향
    /// (`opening::add_openings`)을 거쳐서 왕복시키면 두 쪽이 어긋나는 순간
    /// 깨진다 — 거기가 이 변환에서 제일 미끄러운 자리다 (거울 변환).
    fn opening_roundtrip(o: crate::opening::Opening) -> Value {
        let mut d = Drawing::new_metric();
        crate::opening::add_openings(&mut d, &[o]);
        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 1, "되읽지 못했다: {:?}", back.notes);
        shapes_of(&back.json)[0].clone()
    }

    fn opening(kind: crate::opening::OpeningKind, rotation: f64) -> crate::opening::Opening {
        crate::opening::Opening {
            id: "shape:bimove:door:xyz".into(),
            center: Vec2::new(1200.0, 800.0),
            rotation,
            width: 900.0,
            thickness: 200.0,
            layer: "0".into(),
            kind,
        }
    }

    #[test]
    fn 창을_되읽는다() {
        use crate::opening::OpeningKind;
        let sh = opening_roundtrip(crate::opening::Opening {
            id: "shape:bimove:window:xyz".into(),
            ..opening(OpeningKind::Window, 0.0)
        });
        assert_eq!(sh.get("type").unwrap(), "window");
        // 문·창은 `x,y` 가 **가운데**다 (벽과 다르다 — `opening_of` 주석).
        assert_eq!(sh.get("x").unwrap(), 1200.0);
        assert_eq!(sh.get("y").unwrap(), 800.0);
        let p = sh.get("props").unwrap();
        assert_eq!(p.get("width").unwrap(), 900.0);
        assert_eq!(p.get("thickness").unwrap(), 200.0);
        // 창에는 여는 쪽이 없다.
        assert!(p.get("swing").is_none());
    }

    #[test]
    fn 문의_여는_쪽과_뒤집힘이_왕복한다() {
        use crate::opening::{OpeningKind, Swing};
        // 거울 변환 때문에 네 경우가 서로 다르게 돈다 (`opening.rs` 의 `door`).
        for side in [1.0, -1.0] {
            for flipped in [false, true] {
                let sh = opening_roundtrip(opening(
                    OpeningKind::Door(Swing { side, flipped }),
                    0.0,
                ));
                let p = sh.get("props").unwrap();
                assert_eq!(sh.get("type").unwrap(), "door");
                assert_eq!(
                    p.get("swing").unwrap().as_f64().unwrap(),
                    side,
                    "side {side} flipped {flipped} 에서 여는 쪽이 뒤집혔다",
                );
                assert_eq!(
                    p.get("flipped").unwrap().as_bool().unwrap(),
                    flipped,
                    "side {side} flipped {flipped} 에서 경첩이 뒤집혔다",
                );
            }
        }
    }

    #[test]
    fn 돌아간_문도_왕복한다() {
        use crate::opening::{OpeningKind, Swing};
        for deg in [0.0, 30.0, 90.0, 135.0, -60.0] {
            let rot = (deg as f64).to_radians();
            let sh = opening_roundtrip(opening(
                OpeningKind::Door(Swing { side: 1.0, flipped: false }),
                rot,
            ));
            let p = sh.get("props").unwrap();
            let got = sh.get("rotation").unwrap().as_f64().unwrap();
            // 각도는 ±π 로 감겨 돌아온다 — 같은 방향인지만 본다.
            let diff = ((got - rot + std::f64::consts::PI).rem_euclid(
                std::f64::consts::TAU,
            )) - std::f64::consts::PI;
            assert!(diff.abs() < 1e-9, "{deg}도 가 {got} 으로 돌아왔다");
            assert!((p.get("width").unwrap().as_f64().unwrap() - 900.0).abs() < 1e-9);
            assert!((p.get("thickness").unwrap().as_f64().unwrap() - 200.0).abs() < 1e-9);
            assert!((sh.get("x").unwrap().as_f64().unwrap() - 1200.0).abs() < 1e-9);
            assert!((sh.get("y").unwrap().as_f64().unwrap() - 800.0).abs() < 1e-9);
        }
    }

    #[test]
    fn 못_읽은_것은_센다() {
        // 물량표는 안 연다. 조용히 빼면 그린 게 사라진 걸 아무도 모른다.
        let mut d = Drawing::new_metric();
        let h = d.new_handle();
        d.model.push(Entity::new(
            h,
            EntityKind::Line(cadcraft_doc::Line {
                a: cadcraft_geom::Vec3::new(0.0, 0.0, 0.0),
                b: cadcraft_geom::Vec3::new(10.0, 0.0, 0.0),
            }),
        ));
        crate::identity::stamp_one(&mut d, 0, crate::identity::BOQ, "shape:bimove:boq:xyz");

        let back =
            drawing_to_bimove(&empty_doc(), "page:1", &BTreeSet::new(), &d).expect("되돌리기 실패");
        assert_eq!(back.added, 0);
        assert_eq!(back.notes.len(), 1);
        assert!(back.notes[0].contains("boq"), "무엇을 못 읽었는지 안 적혔다: {:?}", back.notes);
        assert!(shapes_of(&back.json).is_empty(), "못 읽은 것을 넣었다");
    }

    #[test]
    fn ids_in_은_심긴_것만_센다() {
        // id 가 빈 공간은 정체성을 안 심는다 (`identity::stamp_one`).
        let d = drawn(&[zone("shape:a", "방", 5.0), zone("", "이름없음", 3.0)]);
        let got = ids_in(&d);
        assert_eq!(got.len(), 1, "정체성 없는 공간까지 셌다: {got:?}");
        assert!(got.contains("shape:a"));
    }

    #[test]
    fn mtext_꾸밈을_벗긴다() {
        assert_eq!(strip_mtext("방"), "방");
        assert_eq!(strip_mtext("한\\P줄"), "한\n줄");
        assert_eq!(area_of("12.3m²"), Some(12.3));
        assert_eq!(area_of("방"), None);
    }
}
