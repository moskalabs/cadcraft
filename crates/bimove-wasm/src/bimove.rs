//! Bimove 문서를 엔진 `Drawing` 으로 옮긴다.
//!
//! Bimove 에 저장된 도면은 전부 tldraw 스냅샷이었고, 그걸 우리 포맷으로 읽는
//! 길은 Bimove 쪽에 있다(`lib/bimoveDocStore.ts`). 여기서는 **그 포맷을 엔진이
//! 아는 것으로** 바꾼다. 그래야 CADCraft 의 렌더러·선택·스냅·편집이 그대로
//! 붙는다.
//!
//! ── 왜 엔티티로 바꾸는가 ──
//!
//! 그릴 거리(DisplayList)를 직접 만들어 넘기는 길도 있다. 그러면 렌더는 되지만
//! 거기서 끝이다 — pick/snap/exec/undo 가 전부 `Drawing` 에 매달려 있어서,
//! 화면만 나오고 **아무것도 못 만지는** 뷰어가 된다. 엔티티로 바꿔두면 그
//! 길이 전부 열린다.
//!
//! ── 좌표계 ──
//!
//! Bimove 캔버스는 **y 가 아래로** 간다 (DXF 를 들여올 때 y 를 뒤집었다).
//! 엔진은 DXF 와 같은 y-up 이다. 그래서 여기서 y 를 다시 뒤집는다. 글자 회전도
//! 같이 뒤집힌다 (캔버스에서 `rotate(-r)` 로 그리던 것이 엔진에서는 `+r`).
//!
//! ── 안 옮기는 것은 **센다** ──
//!
//! 해치와 Bimove 고유 도형(벽체·문·창·치수·공간·물량표)은 아직 안 옮긴다.
//! 조용히 빼면 "다 나왔다" 로 보이므로 종류별로 세서 `notes` 에 담는다.
//! 벽체 같은 건 CAD 엔티티가 아니라 견적용 주석이라, DXF 엔티티로 밀어넣지
//! 말고 Bimove 쪽 overlay 로 그리는 게 맞다 — 그래서 "아직" 이 아니라
//! "여기서는 안 한다" 가 맞는 것도 섞여 있다.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{Drawing, Entity, EntityKind, Line, MText};
use cadcraft_geom::Vec3;
use serde_json::Value;
use std::collections::BTreeMap;

/// 옮긴 결과.
pub struct BimoveLoad {
    pub drawing: Drawing,
    /// `(id, 이름)` — JS 가 탭을 그리는 데 쓴다.
    pub pages: Vec<(String, String)>,
    /// 옮긴 페이지의 id — 빈 문자열을 넘겼을 때 **무엇이 골라졌는지**.
    pub page_id: String,
    /// 그리지 **않은** 것. 사람이 읽는 문장이다.
    pub notes: Vec<String>,
    pub line_count: usize,
    pub text_count: usize,
}

fn f(v: &Value, key: &str, dflt: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(dflt)
}

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// `"#rrggbb"` → 트루컬러. 못 읽으면 레이어 색을 따른다.
fn color_of(meta: Option<&Value>) -> Color {
    meta.and_then(|m| s(m, "dxfColor"))
        .and_then(Rgb::parse_hex)
        .map_or(Color::ByLayer, Color::True)
}

fn layer_of(meta: Option<&Value>) -> String {
    meta.and_then(|m| s(m, "dxfLayer"))
        .filter(|l| !l.is_empty())
        .unwrap_or("0")
        .to_string()
}

/// 숫자 하나를 읽는다. 앞의 구분자(쉼표·공백)는 건너뛴다.
fn num(b: &[u8], i: &mut usize) -> Option<f64> {
    while *i < b.len() && matches!(b[*i], b',' | b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
    let from = *i;
    if *i < b.len() && (b[*i] == b'-' || b[*i] == b'+') {
        *i += 1;
    }
    let mut digits = 0;
    while *i < b.len() && (b[*i].is_ascii_digit() || b[*i] == b'.') {
        if b[*i].is_ascii_digit() {
            digits += 1;
        }
        *i += 1;
    }
    // 지수 표기(1e-7). toFixed 가 내놓진 않지만 손으로 만든 path 가 섞일 수 있다.
    if digits > 0 && *i < b.len() && (b[*i] == b'e' || b[*i] == b'E') {
        let save = *i;
        *i += 1;
        if *i < b.len() && (b[*i] == b'-' || b[*i] == b'+') {
            *i += 1;
        }
        let mut exp = 0;
        while *i < b.len() && b[*i].is_ascii_digit() {
            exp += 1;
            *i += 1;
        }
        if exp == 0 {
            *i = save;
        }
    }
    if digits == 0 {
        *i = from;
        return None;
    }
    std::str::from_utf8(&b[from..*i]).ok()?.parse().ok()
}

/// 좌표 한 쌍. 못 읽으면 자리를 되돌린다.
fn pair(b: &[u8], i: &mut usize) -> Option<(f64, f64)> {
    let save = *i;
    match (num(b, i), num(b, i)) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => {
            *i = save;
            None
        }
    }
}

/// SVG path 에서 선분을 뽑는다. 돌려주는 건 `(선분들, 못 읽은 명령 수)`.
///
/// Bimove 의 dxfgroup 은 `M x,y L x,y` 를 이어붙인 것뿐이다 — 들여올 때
/// 호·원·스플라인을 이미 선분으로 쪼갰기 때문이다. 그래도 `Z` 와 좌표가
/// 이어지는 꼴(명령 하나 뒤에 점이 여러 개)은 받아주고, **모르는 명령은 센다.**
/// 조용히 넘기면 도면 일부가 사라진 걸 아무도 모른다.
fn parse_path(d: &str) -> (Vec<[f64; 4]>, usize) {
    let b = d.as_bytes();
    let mut i = 0usize;
    let mut segs: Vec<[f64; 4]> = Vec::new();
    let mut unknown = 0usize;
    let mut cur = (0.0f64, 0.0f64);
    let mut start = (0.0f64, 0.0f64);
    let mut have_cur = false;

    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b',' | b'\t' | b'\n' | b'\r' => i += 1,
            b'M' | b'm' | b'L' | b'l' => {
                i += 1;
                let rel = c.is_ascii_lowercase();
                let moving = c == b'M' || c == b'm';
                // 명령 하나 뒤에 점이 여럿 올 수 있다. M 의 두 번째 점부터는
                // L 로 다룬다 (SVG 규칙) — 하나만 읽으면 나머지가 사라진다.
                let mut first = true;
                while let Some((x, y)) = pair(b, &mut i) {
                    let p = if rel && have_cur { (cur.0 + x, cur.1 + y) } else { (x, y) };
                    if moving && first {
                        start = p;
                    } else if have_cur {
                        segs.push([cur.0, cur.1, p.0, p.1]);
                    }
                    cur = p;
                    have_cur = true;
                    first = false;
                }
            }
            b'Z' | b'z' => {
                i += 1;
                if have_cur && (cur.0 - start.0).abs() + (cur.1 - start.1).abs() > 0.0 {
                    segs.push([cur.0, cur.1, start.0, start.1]);
                }
                cur = start;
            }
            _ => {
                // 모르는 명령. 그 인자까지 건너뛴다 (다음 명령 글자 앞까지).
                unknown += 1;
                i += 1;
                while i < b.len() && !b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                // 어디로 갔는지 모르므로 이어 그리지 않는다 — 엉뚱한 선이
                // 생기는 것보다 빠지는 쪽이 눈에 띈다 (게다가 센다).
                have_cur = false;
            }
        }
    }
    (segs, unknown)
}

/// `textsJson` 을 푼다 — 신형(테이블 압축)과 구형(그냥 배열) 둘 다.
///
/// 신형은 `{F:[폰트], C:[색], T:[{…, fi, ci}]}` 다. 폰트·색 문자열이 글자마다
/// 반복되는 걸 테이블로 뺀 것이라, `fi`/`ci` 를 되돌려야 한다. 안 되돌리면
/// 글자가 전부 기본색으로 나온다.
fn parse_texts(json: &str) -> Vec<Value> {
    if json.trim().is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    if let Some(arr) = v.as_array() {
        return arr.clone();
    }
    let fonts = v.get("F").and_then(Value::as_array).cloned().unwrap_or_default();
    let colors = v.get("C").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for mut t in v.get("T").and_then(Value::as_array).cloned().unwrap_or_default() {
        if let Some(obj) = t.as_object_mut() {
            if let Some(fi) = obj.remove("fi").and_then(|n| n.as_u64()) {
                if let Some(name) = fonts.get(fi as usize) {
                    obj.insert("f".into(), name.clone());
                }
            }
            if let Some(ci) = obj.remove("ci").and_then(|n| n.as_u64()) {
                if let Some(cc) = colors.get(ci as usize) {
                    obj.insert("c".into(), cc.clone());
                }
            }
        }
        out.push(t);
    }
    out
}

/// 평문을 MTEXT 본문으로. `\`, `{`, `}` 는 MTEXT 의 제어문자다.
fn to_mtext(plain: &str) -> String {
    let mut out = String::with_capacity(plain.len());
    for ch in plain.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\P"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Bimove 문서(JSON) → 엔진 Drawing.
///
/// `page_id` 가 비어 있으면 첫 페이지. 그 페이지에 직접 달린 도형만 옮긴다 —
/// 그룹 안까지 따라 들어가는 건 다음 일이다 (지금 저장된 도면에는 dxfgroup 이
/// 전부 페이지 직속이다).
pub fn drawing_from_bimove(json: &str, page_id: &str) -> Result<BimoveLoad, String> {
    let doc: Value =
        serde_json::from_str(json).map_err(|e| format!("문서가 JSON 이 아니다: {e}"))?;

    let pages: Vec<(String, String)> = doc
        .get("pages")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some((
                        s(p, "id")?.to_string(),
                        s(p, "name").unwrap_or("페이지").to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let want = if page_id.is_empty() {
        pages.first().map(|(id, _)| id.clone()).unwrap_or_default()
    } else {
        page_id.to_string()
    };

    let shapes = doc.get("shapes").and_then(Value::as_array).cloned().unwrap_or_default();

    let mut drawing = Drawing::new_metric();
    let mut layers: BTreeMap<String, ()> = BTreeMap::new();
    let mut skipped_types: BTreeMap<String, usize> = BTreeMap::new();
    let mut hatch_groups = 0usize;
    let mut unknown_cmds = 0usize;
    let mut line_count = 0usize;
    let mut text_count = 0usize;

    for sh in &shapes {
        let parent = s(sh, "parentId").unwrap_or("");
        // parentId 가 빈 도형은 옛 저장본이다 (페이지가 하나뿐이었다).
        if !want.is_empty() && !parent.is_empty() && parent != want {
            continue;
        }
        let kind = s(sh, "type").unwrap_or("");
        let props = sh.get("props").cloned().unwrap_or(Value::Null);
        let meta = sh.get("meta");

        if kind != "dxfgroup" {
            *skipped_types.entry(kind.to_string()).or_insert(0) += 1;
            continue;
        }

        let gx = f(sh, "x", 0.0);
        let gy = f(sh, "y", 0.0);
        let layer = layer_of(meta);
        let color = color_of(meta);
        layers.entry(layer.clone()).or_insert(());

        if let Some(d) = s(&props, "pathData") {
            let (segs, unknown) = parse_path(d);
            unknown_cmds += unknown;
            for sg in segs {
                let h = drawing.new_handle();
                let mut e = Entity::new(
                    h,
                    EntityKind::Line(Line {
                        // y 를 뒤집는다 — 캔버스는 아래로, 엔진은 위로 간다
                        a: Vec3::new(gx + sg[0], -(gy + sg[1]), 0.0),
                        b: Vec3::new(gx + sg[2], -(gy + sg[3]), 0.0),
                    }),
                );
                e.common.layer = layer.clone();
                e.common.color = color;
                drawing.model.push(e);
                line_count += 1;
            }
        }

        for t in parse_texts(s(&props, "textsJson").unwrap_or("")) {
            let value = s(&t, "t").unwrap_or("");
            if value.is_empty() {
                continue;
            }
            let h = drawing.new_handle();
            let attach = f(&t, "ap", 1.0).round().clamp(1.0, 9.0) as u8;
            let mut e = Entity::new(
                h,
                EntityKind::MText(MText {
                    insert: Vec3::new(gx + f(&t, "x", 0.0), -(gy + f(&t, "y", 0.0)), 0.0),
                    height: f(&t, "h", 2.5).max(1e-6),
                    width: f(&t, "mw", 0.0).max(0.0),
                    attach,
                    // 캔버스에서 `rotate(-r)` 로 그리던 것 = 엔진에서 +r
                    rotation: f(&t, "r", 0.0).to_radians(),
                    style: "Standard".into(),
                    contents: to_mtext(value),
                    line_spacing: 1.0,
                    line_spacing_exact: false,
                    columns: None,
                }),
            );
            e.common.layer = layer.clone();
            e.common.color =
                s(&t, "c").and_then(Rgb::parse_hex).map_or(Color::ByLayer, Color::True);
            drawing.model.push(e);
            text_count += 1;
        }

        if s(&props, "hatchesJson").is_some_and(|h| !h.trim().is_empty()) {
            hatch_groups += 1;
        }
    }

    // 쓰인 레이어를 만들어 둔다. 없으면 엔진이 레이어를 못 찾아 "0" 으로만 본다.
    for name in layers.keys() {
        if name != "0" && !drawing.layers.iter().any(|l| &l.name == name) {
            if let Some(base) = drawing.layers.first().cloned() {
                drawing.layers.push(cadcraft_doc::Layer { name: name.clone(), ..base });
            }
        }
    }

    let mut notes = Vec::new();
    if hatch_groups > 0 {
        notes.push(format!("해치가 든 그룹 {hatch_groups}개 — 해치는 아직 안 그린다"));
    }
    if unknown_cmds > 0 {
        notes.push(format!("path 명령 {unknown_cmds}개를 못 읽었다 (M/L/Z 만 다룬다)"));
    }
    for (k, n) in &skipped_types {
        notes.push(format!("{k} {n}개 — 여기서 안 그린다 (Bimove overlay 몫)"));
    }

    Ok(BimoveLoad { drawing, pages, page_id: want, notes, line_count, text_count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 직선_하나() {
        let (segs, unknown) = parse_path("M0,0L100,50");
        assert_eq!(segs, vec![[0.0, 0.0, 100.0, 50.0]]);
        assert_eq!(unknown, 0);
    }

    #[test]
    fn 떨어진_선분들() {
        // Bimove 가 내놓는 꼴이다 — M/L 짝이 이어붙어 있고 서로 안 붙어 있다.
        let (segs, _) = parse_path("M0,0L10,0M20,0L30,0");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[1], [20.0, 0.0, 30.0, 0.0]);
    }

    #[test]
    fn 음수와_소수() {
        let (segs, _) = parse_path("M-1.5,-2.5L3.25,-0.75");
        assert_eq!(segs, vec![[-1.5, -2.5, 3.25, -0.75]]);
    }

    #[test]
    fn 점이_여럿인_l() {
        // SVG 는 L 뒤에 점을 여러 개 쓸 수 있다. 하나만 읽으면 **나머지가
        // 조용히 사라진다.**
        let (segs, _) = parse_path("M0,0L10,0 20,0 30,0");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[2], [20.0, 0.0, 30.0, 0.0]);
    }

    #[test]
    fn m_뒤에_점이_여럿이면_두번째부터_선이다() {
        let (segs, _) = parse_path("M0,0 10,0 20,0");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0], [0.0, 0.0, 10.0, 0.0]);
    }

    #[test]
    fn 닫는_z() {
        let (segs, _) = parse_path("M0,0L10,0L10,10Z");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[2], [10.0, 10.0, 0.0, 0.0]);
    }

    #[test]
    fn z_가_이미_제자리면_선을_더하지_않는다() {
        let (segs, _) = parse_path("M0,0L10,0L0,0Z");
        assert_eq!(segs.len(), 2);
    }

    #[test]
    fn 모르는_명령을_센다() {
        // 조용히 넘기면 도면 일부가 사라진 걸 아무도 모른다.
        let (segs, unknown) = parse_path("M0,0L10,0C1,2 3,4 5,6L20,20M30,0L40,0");
        assert_eq!(unknown, 1);
        // C 앞까지는 그렸고, C 뒤로는 어디인지 모르므로 이어 그리지 않는다.
        // 다음 M 부터 다시 그린다.
        assert_eq!(segs, vec![[0.0, 0.0, 10.0, 0.0], [30.0, 0.0, 40.0, 0.0]]);
    }

    #[test]
    fn 빈_path() {
        assert_eq!(parse_path("").0.len(), 0);
        assert_eq!(parse_path("Z").0.len(), 0);
        assert_eq!(parse_path("M0,0").0.len(), 0);
    }

    fn doc_json(shapes: &str) -> String {
        format!(
            r#"{{"version":1,"pages":[{{"id":"page:main","name":"모형","index":"a1"}}],
                 "shapes":[{shapes}],"records":{{}},"skipped":0,
                 "source":{{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":1}}}}"#
        )
    }

    #[test]
    fn dxfgroup_이_선으로_들어온다() {
        let j = doc_json(
            r##"{"id":"shape:g","type":"dxfgroup","x":100,"y":200,"rotation":0,"index":"a1",
                "parentId":"page:main","props":{"pathData":"M0,0L10,0","textsJson":""},
                "meta":{"dxfLayer":"벽체","dxfColor":"#ff0000"}}"##,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.line_count, 1);
        assert_eq!(r.page_id, "page:main");

        let e = r.drawing.model.iter().next().unwrap();
        match &e.kind {
            EntityKind::Line(l) => {
                // 도형 위치가 더해지고 **y 가 뒤집혀** 있다
                assert_eq!((l.a.x, l.a.y), (100.0, -200.0));
                assert_eq!((l.b.x, l.b.y), (110.0, -200.0));
            }
            k => panic!("Line 이 아니다: {}", k.type_name()),
        }
        assert_eq!(e.common.layer, "벽체");
        assert_eq!(e.common.color, Color::True(Rgb(0xff, 0, 0)));
    }

    #[test]
    fn 글자가_mtext_로_들어온다() {
        let j = doc_json(
            r#"{"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1",
                "parentId":"page:main","props":{"pathData":"",
                "textsJson":"[{\"x\":5,\"y\":7,\"t\":\"거실\",\"h\":2.5,\"r\":90,\"ap\":5}]"}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.text_count, 1);
        match &r.drawing.model.iter().next().unwrap().kind {
            EntityKind::MText(m) => {
                assert_eq!(m.contents, "거실");
                assert_eq!((m.insert.x, m.insert.y), (5.0, -7.0));
                assert_eq!(m.attach, 5);
                assert!((m.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
            }
            k => panic!("MText 가 아니다: {}", k.type_name()),
        }
    }

    #[test]
    fn 압축된_textsjson_의_폰트와_색을_되돌린다() {
        let j = doc_json(
            r##"{"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1",
                "parentId":"page:main","props":{"pathData":"",
                "textsJson":"{\"F\":[\"맑은 고딕\"],\"C\":[\"#00ff00\"],\"T\":[{\"x\":0,\"y\":0,\"t\":\"가\",\"h\":2,\"fi\":0,\"ci\":0}]}"}}"##,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.text_count, 1);
        assert_eq!(
            r.drawing.model.iter().next().unwrap().common.color,
            Color::True(Rgb(0, 0xff, 0))
        );
    }

    #[test]
    fn 줄바꿈이_p_로_바뀐다() {
        assert_eq!(to_mtext("가\n나"), "가\\P나");
        // MTEXT 제어문자는 그대로 두면 서식 명령으로 먹힌다
        assert_eq!(to_mtext("a{b}c\\d"), "a\\{b\\}c\\\\d");
    }

    #[test]
    fn 안_그린_것을_센다() {
        // 조용히 빼면 "다 나왔다" 로 보인다.
        let j = doc_json(
            r#"{"id":"shape:w","type":"wall","x":0,"y":0,"rotation":0,"index":"a1",
                "parentId":"page:main","props":{"thickness":20}},
               {"id":"shape:z","type":"zone","x":0,"y":0,"rotation":0,"index":"a2",
                "parentId":"page:main","props":{}},
               {"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a3",
                "parentId":"page:main","props":{"pathData":"M0,0L1,0","hatchesJson":"[{}]"}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.line_count, 1);
        let all = r.notes.join(" | ");
        assert!(all.contains("wall 1개"), "{all}");
        assert!(all.contains("zone 1개"), "{all}");
        assert!(all.contains("해치"), "{all}");
    }

    #[test]
    fn 다른_페이지의_도형은_안_가져온다() {
        let j = r#"{"version":1,
          "pages":[{"id":"page:a","name":"모형","index":"a1"},{"id":"page:b","name":"배치1","index":"a2"}],
          "shapes":[
            {"id":"shape:1","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1","parentId":"page:a","props":{"pathData":"M0,0L1,0"}},
            {"id":"shape:2","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1","parentId":"page:b","props":{"pathData":"M0,0L1,0M2,0L3,0"}}],
          "records":{},"skipped":0,
          "source":{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":2}}"#;
        assert_eq!(drawing_from_bimove(j, "").unwrap().line_count, 1);
        assert_eq!(drawing_from_bimove(j, "page:b").unwrap().line_count, 2);
        assert_eq!(drawing_from_bimove(j, "").unwrap().pages.len(), 2);
    }

    #[test]
    fn 레이어를_만들어_둔다() {
        let j = doc_json(
            r#"{"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1",
                "parentId":"page:main","props":{"pathData":"M0,0L1,0"},
                "meta":{"dxfLayer":"A-WALL"}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert!(r.drawing.layers.iter().any(|l| l.name == "A-WALL"));
    }

    #[test]
    fn 말이_안_되는_입력에도_터지지_않는다() {
        assert!(drawing_from_bimove("", "").is_err());
        assert_eq!(drawing_from_bimove("42", "").unwrap().line_count, 0);
        assert_eq!(drawing_from_bimove("{}", "").unwrap().line_count, 0);
        // props 가 없거나 모양이 다른 도형
        let j = doc_json(r#"{"id":"x","type":"dxfgroup"}"#);
        assert_eq!(drawing_from_bimove(&j, "").unwrap().line_count, 0);
    }
}
