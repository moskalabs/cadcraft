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
//! 남은 Bimove 고유 도형(치수·공간·물량표)은 아직 안 옮긴다. 조용히 빼면
//! "다 나왔다" 로 보이므로 종류별로 세서 `notes` 에 담는다.
//!
//! 벽체는 **옮긴다** (`crate::wall`). 한때 "벽은 CAD 엔티티가 아니라 견적용
//! 주석이라 overlay 로 그리는 게 맞다" 고 적어 뒀는데, 그건 tldraw 를 남겨
//! 두는 전제에서만 맞는 말이었다. 걷어낼 거라면 벽도 엔진이 알아야 하고,
//! 마침 **MLINE 이 평면도의 벽 그 자체**다 — 중심선·두께·miter·캡·속 채움이
//! 전부 들어 있다. 손으로 miter 를 다시 계산할 이유가 없다.
//!
//! 문·창도 **옮긴다** (`crate::opening`). 벽을 가려서 갭을 뚫는 것이라 벽보다
//! 나중에 넣어야 한다 — 그 차례가 거기서 지켜진다.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::library::PatternLine;
use cadcraft_doc::{Drawing, Entity, EntityKind, Hatch, HatchLoop, Line, LwPolyline, MText};
use cadcraft_geom::{PolyVertex, Vec2, Vec3};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::opening::{self, Opening, OpeningKind, Swing};
use crate::wall::WallSeg;

/// 옮긴 결과.
pub struct BimoveLoad {
    pub drawing: Drawing,
    /// `(id, 이름)` — JS 가 탭을 그리는 데 쓴다.
    pub pages: Vec<(String, String)>,
    /// 옮긴 페이지의 id — 빈 문자열을 넘겼을 때 **무엇이 골라졌는지**.
    pub page_id: String,
    /// 그리지 **않은** 것. 사람이 읽는 문장이다.
    pub notes: Vec<String>,
    /// 만든 엔티티 수 (선 + 폴리라인).
    pub line_count: usize,
    pub text_count: usize,
    /// 이어 붙이기 전의 선분 수. `line_count` 와 견주면 얼마나 줄었는지 보인다.
    pub segment_count: usize,
    pub hatch_count: usize,
    /// 읽은 벽체 **도형** 수.
    pub wall_count: usize,
    /// 만든 MLINE 수. `wall_count` 보다 **작아야** 묶기가 듣는 것이다 —
    /// 같으면 이어진 벽을 하나도 못 묶었다는 뜻이고, 그러면 코너마다 캡이 선다.
    pub wall_mline_count: usize,
    /// 읽은 문·창 **도형** 수. 하나가 엔티티 여럿(가림 + 잼브 + 문짝 + 호)이
    /// 되므로 엔티티 수와는 다르다.
    pub opening_count: usize,
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

/// 끝점이 같다고 볼 오차. 들여올 때 좌표를 소수 첫째 자리로 반올림했으므로
/// (`toFixed(1)`) 그보다 작게 잡으면 붙어 있던 것도 안 붙는다.
const JOIN_EPS: f64 = 0.05;

/// 이어지는 선분을 **한 줄로 묶는다.**
///
/// Bimove 는 폴리라인을 `M a L b M b L c …` 로 쪼개 저장한다. 그대로 엔티티로
/// 만들면 40점짜리 폴리라인 하나가 선 39개가 된다. 그게 그대로 프레임 시간이다
/// — 래스터라이저는 프리미티브마다 path 를 만들어 한 번씩 stroke 하므로,
/// 개수가 곧 비용이다 (60만 선분짜리에서 한 장에 900ms 를 쟀다).
///
/// 들어온 차례대로 보면서 앞 선분의 끝에서 시작하는 것만 잇는다. 정렬하거나
/// 멀리서 찾아오지 않는다 — 들여올 때 같은 폴리라인의 선분이 이미 붙어 있고,
/// 욕심내면 원래 떨어져 있던 선이 이어 붙어 **없던 선이 생긴다.**
fn chain(segs: &[[f64; 4]]) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    for sg in segs {
        let (a, b) = ((sg[0], sg[1]), (sg[2], sg[3]));
        match cur.last() {
            Some(&last) if (last.0 - a.0).abs() <= JOIN_EPS && (last.1 - a.1).abs() <= JOIN_EPS => {
                cur.push(b);
            }
            _ => {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                cur.push(a);
                cur.push(b);
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

// ── 해치 ──
//
// Bimove 는 해치를 `hatchesJson` 에 이렇게 싣는다 (dxf.ts 의 packHatchPattern):
//
//   { d: 경계 SVG path(그룹 로컬 좌표), p: 패턴이름, s: 축척, a: 각도,
//     sp?: 정의선 간격, n?: 정의선 수, dl?: [{a,s,d?}], c?: 색, f?: 1이면 솔리드,
//     hs?: 스타일, dim?: 해치 바운딩박스 최대 변 }
//
// 엔진의 `Hatch` 가 경계·패턴·스타일을 다 받으므로 **패턴을 우리가 그릴 필요가
// 없다.** 정의선(`dl`)을 `pattern_lines` 로 넘기면 엔진이 경계에 맞춰 잘라 그린다.
// 지금 Bimove 가 SVG `<pattern>` 으로 흉내 내던 것보다 충실하다.
//
// 단위: `dl[].s` 는 **캔버스 px** 다 (들여올 때 autoScale 이 곱해진 뒤 잰 값).
// 우리가 만드는 도형도 같은 px 공간이라 `scale: 1` 로 두면 맞아떨어진다.
// DXF 의 `patternScale` 을 그대로 쓰면 안 된다 — 그건 원본 도면 단위다.

/// 간격을 해치 크기에 견줘 가둔다.
///
/// 간격이 아주 촘촘하면(도면 전체 축척에서 1px 수준) 패턴이 통짜 회색으로
/// 뭉개지고, 엔진이 선을 수만 개 만든다. 반대로 너무 성기면 해치 안에 선이
/// 한 줄도 안 들어간다. 지금 Bimove 가 쓰는 것과 **같은 울타리**를 쓴다
/// (해치 한 변에 4~60번 반복).
fn clamp_spacing(sp: f64, dim: f64) -> f64 {
    let dim = if dim.is_finite() && dim > 1.0 { dim } else { 400.0 };
    sp.clamp(dim / 60.0, dim / 4.0)
}

/// 닫힌 부분경로들을 해치 고리로. 넓이가 가장 큰 것을 바깥 고리로 본다.
fn hatch_loops(runs: Vec<Vec<(f64, f64)>>, at: impl Fn((f64, f64)) -> Vec2) -> Vec<HatchLoop> {
    let mut areas: Vec<(usize, f64)> = Vec::new();
    let mut loops: Vec<HatchLoop> = Vec::new();
    for (i, mut run) in runs.into_iter().enumerate() {
        // 닫힌 path 는 마지막 점이 첫 점과 같다. 고리는 그걸 되풀이하지 않는다.
        if run.len() > 2 {
            if let (Some(&a), Some(&b)) = (run.first(), run.last()) {
                if (a.0 - b.0).abs() <= JOIN_EPS && (a.1 - b.1).abs() <= JOIN_EPS {
                    run.pop();
                }
            }
        }
        if run.len() < 3 {
            continue;
        }
        // 신발끈 공식 — 부호는 감는 방향이라 크기만 본다.
        let mut area2 = 0.0;
        for k in 0..run.len() {
            let (x1, y1) = run[k];
            let (x2, y2) = run[(k + 1) % run.len()];
            area2 += x1 * y2 - x2 * y1;
        }
        areas.push((i, area2.abs()));
        loops.push(HatchLoop {
            vertices: run.into_iter().map(|p| PolyVertex::new(at(p))).collect(),
            outer: false,
            sources: Vec::new(),
        });
    }
    // 바깥 고리가 하나도 없으면 아무것도 안 칠해질 수 있다. 가장 큰 것을 세운다.
    if let Some((best, _)) = areas.iter().enumerate().max_by(|a, b| a.1 .1.total_cmp(&b.1 .1)) {
        if let Some(l) = loops.get_mut(best) {
            l.outer = true;
        }
    }
    loops
}

/// `hatchesJson` 의 해치들을 도면에 넣는다. 넣은 개수를 돌려준다.
fn add_hatches(
    drawing: &mut Drawing, json: &str, gx: f64, gy: f64, layer: &str, group_color: Color,
) -> usize {
    if json.trim().is_empty() {
        return 0;
    }
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else {
        return 0;
    };
    // y 를 뒤집는다 — 다른 도형과 같은 변환
    let at = |p: (f64, f64)| Vec2::new(gx + p.0, -(gy + p.1));
    let mut made = 0;

    for h in &items {
        let Some(d) = s(h, "d") else { continue };
        let (segs, _) = parse_path(d);
        let loops = hatch_loops(chain(&segs), at);
        if loops.is_empty() {
            continue;
        }

        let name = s(h, "p").unwrap_or("").to_ascii_uppercase();
        let solid = f(h, "f", 0.0) != 0.0 || name == "SOLID";
        let dim = f(h, "dim", 400.0);

        // 정의선. 캔버스에서 y 가 뒤집히므로 각도의 부호도 뒤집는다
        // (지금 SVG 렌더러가 `rotate(-angle)` 하는 것과 같은 이유다).
        let mut lines: Vec<PatternLine> = Vec::new();
        if !solid {
            if let Some(Value::Array(dl)) = h.get("dl") {
                for e in dl {
                    let sp = clamp_spacing(f(e, "s", 0.0), dim);
                    if !(sp.is_finite() && sp > 0.0) {
                        continue;
                    }
                    lines.push(PatternLine {
                        angle: -f(e, "a", 0.0),
                        origin: (0.0, 0.0),
                        delta: (0.0, sp),
                        dashes: e
                            .get("d")
                            .and_then(Value::as_array)
                            .map(|a| a.iter().filter_map(Value::as_f64).collect())
                            .unwrap_or_default(),
                    });
                }
            }
            if lines.is_empty() {
                // 정의선이 안 실린 옛 데이터. 이름만 가지고는 px 간격을 알 수
                // 없어서, 지금 Bimove 가 쓰는 추정과 같은 값을 쓴다.
                //
                // **비워 두면 안 된다.** 비면 엔진이 표준 라이브러리의 같은
                // 이름 패턴을 `scale` 로 그리는데, 우리 scale 은 1 이고
                // 라이브러리 ANSI31 의 간격은 0.125 **도면 단위**다 — 우리
                // 단위가 캔버스 px 라 0.125px 간격이 되어 통짜로 뭉개진다.
                let est = (dim / 40.0).max(10.0) * f(h, "s", 1.0).max(0.5);
                lines.push(PatternLine {
                    angle: -f(h, "a", 45.0),
                    origin: (0.0, 0.0),
                    delta: (0.0, clamp_spacing(est, dim)),
                    dashes: Vec::new(),
                });
            }
        }

        let handle = drawing.new_handle();
        let mut e = Entity::new(
            handle,
            EntityKind::Hatch(Hatch {
                pattern: if name.is_empty() { "ANSI31".into() } else { name },
                solid,
                loops,
                // 간격·각도를 이미 우리 공간으로 옮겨 실었다. 여기서 또 돌리거나
                // 키우면 두 번 먹는다.
                scale: 1.0,
                angle: 0.0,
                associative: false,
                style: f(h, "hs", 0.0).clamp(0.0, 2.0) as u8,
                elevation: 0.0,
                gradient: None,
                origin: Vec2::ZERO,
                background: None,
                pattern_lines: lines,
            }),
        );
        e.common.layer = layer.to_string();
        e.common.color = s(h, "c").and_then(Rgb::parse_hex).map_or(group_color, Color::True);
        drawing.model.push(e);
        made += 1;
    }
    made
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
/// 벽체 도형 하나 → 중심선 선분.
///
/// 돌려주는 좌표는 **엔진** 것이다 (y-up). 캔버스에서 회전을 먹인 뒤 두 점의
/// y 를 같이 뒤집는다 — 뒤집기는 거울이므로 회전각을 따로 고칠 필요가 없다.
///
/// `None` 이면 못 쓰는 벽이다 (두께 0, 길이 0, NaN). 부른 쪽이 **따로 세서**
/// `notes` 에 담는다 — 그냥 빼면 도형이 하나 사라진 걸 아무도 모른다.
fn wall_seg(sh: &Value, props: &Value, meta: Option<&Value>) -> Option<WallSeg> {
    let (x2, y2) = (f(props, "x2", 0.0), f(props, "y2", 0.0));
    let thickness = f(props, "thickness", 0.0);
    if !(thickness > 0.0) || !(x2.hypot(y2) > 1e-9) {
        return None;
    }
    let (gx, gy) = (f(sh, "x", 0.0), f(sh, "y", 0.0));
    let rot = f(sh, "rotation", 0.0);
    let (c, s_) = (rot.cos(), rot.sin());
    let (ex, ey) = (gx + x2 * c - y2 * s_, gy + x2 * s_ + y2 * c);
    if !(gx.is_finite() && gy.is_finite() && ex.is_finite() && ey.is_finite()) {
        return None;
    }

    let hex = |key: &str| meta.and_then(|m| s(m, key)).and_then(Rgb::parse_hex);
    Some(WallSeg {
        a: Vec2::new(gx, -gy),
        b: Vec2::new(ex, -ey),
        thickness,
        layer: layer_of(meta),
        // 윤곽선은 못 주면 `ByLayer` 로 둔다. 엔진이 배경을 보고 잉크를 고르므로
        // 어두운 배경에서도 읽힌다 — 여기서 `#222` 로 박으면 다크에서 묻는다.
        color: hex("stroke").map_or_else(|| color_of(meta), Color::True),
        // 속은 **흰색**이다. 평면도의 벽은 가는 윤곽선 두 줄 + 빈 속이 맞다
        // (`WallShape.tsx` 의 `rawFill` 과 같은 기본값).
        //
        // 다크 배경에서는 이게 하얗게 뜬다. 고치려면 배경색이 여기까지
        // 내려와야 하는데 `drawing_from_bimove` 는 그걸 모른다 — 색을 짐작하지
        // 말고, 배경을 넘기게 될 때 같이 손대는 게 맞다.
        fill: Some(hex("fill").map_or(Color::True(Rgb(255, 255, 255)), Color::True)),
    })
}

/// 문·창 도형 하나를 읽는다.
///
/// 벽과 달리 `x,y` 가 **가운데**다 (벽선에 스냅된 그 점) — `rotation` 이 벽
/// 방향이고, 제 좌표계는 x 가 벽을 따라, y 가 벽을 가로지른다. 돌리기와 y
/// 뒤집기는 `opening` 안에서 한다: 호의 방향이 거울에 뒤집히므로 그 계산을
/// 한 자리에 모아 둬야 한다.
///
/// `None` 이면 못 쓰는 것이다 (폭 0, 두께 0, NaN). 벽과 같이 **따로 세서**
/// `notes` 에 담는다.
fn opening_of(sh: &Value, props: &Value, meta: Option<&Value>, kind: &str) -> Option<Opening> {
    let width = f(props, "width", 0.0);
    let thickness = f(props, "thickness", 0.0);
    let center = Vec2::new(f(sh, "x", 0.0), f(sh, "y", 0.0));
    let o = Opening {
        center,
        rotation: f(sh, "rotation", 0.0),
        width,
        thickness,
        layer: layer_of(meta),
        kind: if kind == "door" {
            OpeningKind::Door(Swing {
                // 0 이 들어오면 문짝이 벽 위에 겹쳐 눕는다. 화면의 기본값이 +1 이다.
                side: if f(props, "swing", 1.0) < 0.0 { -1.0 } else { 1.0 },
                flipped: props.get("flipped").and_then(Value::as_bool).unwrap_or(false),
            })
        } else {
            OpeningKind::Window
        },
    };
    opening::usable(&o).then_some(o)
}

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
    let mut hatch_count = 0usize;
    let mut unknown_cmds = 0usize;
    let mut line_count = 0usize;
    let mut text_count = 0usize;
    let mut segment_count = 0usize;
    // 벽은 **모아 두고 한꺼번에** 넣는다. 이어진 벽을 한 MLINE 으로 묶으려면
    // 이웃을 알아야 하고, 이웃은 도형을 다 읽은 뒤에야 다 보인다.
    let mut walls: Vec<WallSeg> = Vec::new();
    // 본 벽체 도형 수. `walls.len()` 과 다를 수 있다 — 두께 0 이나 길이 0 짜리는
    // 그릴 수가 없다. 그 차이를 세지 않으면 도형이 조용히 사라진다.
    let mut wall_seen = 0usize;
    // 문·창도 모아 둔다 — 벽을 다 넣은 **뒤에** 넣어야 벽을 가린다 (`opening.rs`).
    let mut openings: Vec<Opening> = Vec::new();
    let mut opening_seen = 0usize;

    for sh in &shapes {
        let parent = s(sh, "parentId").unwrap_or("");
        // parentId 가 빈 도형은 옛 저장본이다 (페이지가 하나뿐이었다).
        if !want.is_empty() && !parent.is_empty() && parent != want {
            continue;
        }
        let kind = s(sh, "type").unwrap_or("");
        let props = sh.get("props").cloned().unwrap_or(Value::Null);
        let meta = sh.get("meta");

        if kind == "wall" {
            wall_seen += 1;
            if let Some(w) = wall_seg(sh, &props, meta) {
                layers.entry(w.layer.clone()).or_insert(());
                walls.push(w);
            }
            continue;
        }

        if kind == "door" || kind == "window" {
            opening_seen += 1;
            if let Some(o) = opening_of(sh, &props, meta, kind) {
                layers.entry(o.layer.clone()).or_insert(());
                openings.push(o);
            }
            continue;
        }

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
            segment_count += segs.len();
            // y 를 뒤집는다 — 캔버스는 아래로, 엔진은 위로 간다
            let at = |p: (f64, f64)| Vec2::new(gx + p.0, -(gy + p.1));
            for run in chain(&segs) {
                let h = drawing.new_handle();
                let kind = if run.len() == 2 {
                    EntityKind::Line(Line {
                        a: { let p = at(run[0]); Vec3::new(p.x, p.y, 0.0) },
                        b: { let p = at(run[1]); Vec3::new(p.x, p.y, 0.0) },
                    })
                } else {
                    EntityKind::LwPolyline(LwPolyline {
                        vertices: run.iter().map(|&p| PolyVertex::new(at(p))).collect(),
                        closed: false,
                        const_width: 0.0,
                        elevation: 0.0,
                        plinegen: false,
                    })
                };
                let mut e = Entity::new(h, kind);
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

        hatch_count +=
            add_hatches(&mut drawing, s(&props, "hatchesJson").unwrap_or(""), gx, gy, &layer, color);
    }

    // 벽은 맨 마지막에 넣는다 — 이웃을 다 본 뒤에야 줄기로 묶을 수 있다.
    let wall_count = wall_seen;
    let wall_dropped = wall_seen - walls.len();
    let wall_mline_count = crate::wall::add_walls(&mut drawing, &walls);

    // 문·창은 그 **다음**이다. 벽을 가려서 갭을 뚫는 것이라 차례가 뒤집히면
    // 벽이 문 위에 덮인다.
    let opening_count = opening_seen;
    let opening_dropped = opening_seen - openings.len();
    opening::add_openings(&mut drawing, &openings);

    // 쓰인 레이어를 만들어 둔다. 없으면 엔진이 레이어를 못 찾아 "0" 으로만 본다.
    for name in layers.keys() {
        if name != "0" && !drawing.layers.iter().any(|l| &l.name == name) {
            if let Some(base) = drawing.layers.first().cloned() {
                drawing.layers.push(cadcraft_doc::Layer { name: name.clone(), ..base });
            }
        }
    }

    let mut notes = Vec::new();
    if unknown_cmds > 0 {
        notes.push(format!("path 명령 {unknown_cmds}개를 못 읽었다 (M/L/Z 만 다룬다)"));
    }
    if opening_dropped > 0 {
        notes.push(format!("문·창 {opening_dropped}개를 못 그렸다 (폭이나 두께가 0)"));
    }
    if wall_dropped > 0 {
        notes.push(format!("벽체 {wall_dropped}개를 못 그렸다 (두께나 길이가 0)"));
    }
    for (k, n) in &skipped_types {
        notes.push(format!("{k} {n}개 — 여기서 안 그린다 (Bimove overlay 몫)"));
    }

    Ok(BimoveLoad {
        drawing,
        pages,
        page_id: want,
        notes,
        line_count,
        text_count,
        segment_count,
        hatch_count,
        wall_count,
        wall_mline_count,
        opening_count,
    })
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

    #[test]
    fn 이어지는_선분이_한_줄로_묶인다() {
        // Bimove 는 폴리라인을 M/L 짝으로 쪼개 저장한다. 그대로 두면 40점짜리
        // 폴리라인 하나가 엔티티 39개가 되고, 그 개수가 그대로 프레임 시간이다.
        let runs = chain(&[[0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 2.0, 0.0], [2.0, 0.0, 2.0, 1.0]]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0], vec![(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (2.0, 1.0)]);
    }

    #[test]
    fn 떨어진_선분은_잇지_않는다() {
        // 욕심내서 이으면 **없던 선이 생긴다.** 도면에서 그건 치명적이다.
        let runs = chain(&[[0.0, 0.0, 1.0, 0.0], [5.0, 5.0, 6.0, 5.0]]);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], vec![(0.0, 0.0), (1.0, 0.0)]);
        assert_eq!(runs[1], vec![(5.0, 5.0), (6.0, 5.0)]);
    }

    #[test]
    fn 반올림_오차만큼은_같은_점으로_본다() {
        // 들여올 때 소수 첫째 자리로 반올림했다(toFixed(1)). 정확히 같기를
        // 바라면 붙어 있던 폴리라인이 안 이어진다.
        let runs = chain(&[[0.0, 0.0, 1.0, 0.0], [1.02, 0.0, 2.0, 0.0]]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len(), 3);
    }

    #[test]
    fn 이어진_것은_폴리라인_떨어진_것은_선() {
        let j = doc_json(
            r#"{"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1",
                "parentId":"page:main",
                "props":{"pathData":"M0,0L1,0M1,0L2,0M9,9L10,9"}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.segment_count, 3);
        assert_eq!(r.line_count, 2, "엔티티 2개(폴리라인 1 + 선 1)여야 한다");
        let kinds: Vec<&str> = r.drawing.model.iter().map(|e| e.kind.type_name()).collect();
        assert!(kinds.contains(&"Polyline"), "{kinds:?}");
        assert!(kinds.contains(&"Line"), "{kinds:?}");
    }

    #[test]
    fn 묶여도_점은_하나도_안_잃는다() {
        // 이어 붙이기가 좌표를 먹으면 도면이 조용히 달라진다.
        let j = doc_json(
            r#"{"id":"shape:g","type":"dxfgroup","x":100,"y":50,"rotation":0,"index":"a1",
                "parentId":"page:main",
                "props":{"pathData":"M0,0L10,0M10,0L10,20"}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        match &r.drawing.model.iter().next().unwrap().kind {
            EntityKind::LwPolyline(p) => {
                let pts: Vec<(f64, f64)> = p.vertices.iter().map(|v| (v.p.x, v.p.y)).collect();
                // 도형 위치가 더해지고 y 가 뒤집힌다
                assert_eq!(pts, vec![(100.0, -50.0), (110.0, -50.0), (110.0, -70.0)]);
            }
            k => panic!("폴리라인이 아니다: {}", k.type_name()),
        }
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
            r#"{"id":"shape:z","type":"zone","x":0,"y":0,"rotation":0,"index":"a2",
                "parentId":"page:main","props":{}},
               {"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a3",
                "parentId":"page:main","props":{"pathData":"M0,0L1,0","hatchesJson":"[{}]"}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.line_count, 1);
        let all = r.notes.join(" | ");
        assert!(all.contains("zone 1개"), "{all}");
        // 해치는 이제 **그린다** — 못 그리는 것 목록에 있으면 안 된다
        assert!(!all.contains("해치"), "{all}");
        // 벽체도 이제 그린다. 전엔 여기 "wall 1개" 가 있었다.
        assert!(!all.contains("wall"), "{all}");
        // 문·창도 그린다. 이 목록은 줄어들기만 한다.
        assert!(!all.contains("door") && !all.contains("window"), "{all}");
    }

    // ── 벽체 ──

    /// 벽체 도형 하나. `x2`/`y2` 는 **제 시작점 기준** 상대 좌표다.
    fn wall_json(i: u32, x: f64, y: f64, x2: f64, y2: f64, t: f64) -> String {
        format!(
            r#"{{"id":"shape:w{i}","type":"wall","x":{x},"y":{y},"rotation":0,"index":"a{i}",
               "parentId":"page:main","props":{{"x2":{x2},"y2":{y2},"thickness":{t}}}}}"#
        )
    }

    #[test]
    fn 벽을_mline_으로_넣는다() {
        let j = doc_json(&wall_json(1, 0.0, 0.0, 100.0, 0.0, 20.0));
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.wall_count, 1);
        assert_eq!(r.wall_mline_count, 1);
        let mls: Vec<_> = r
            .drawing
            .model
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::MLine(m) => Some(m),
                _ => None,
            })
            .collect();
        assert_eq!(mls.len(), 1);
        assert_eq!(mls[0].scale, 20.0);
    }

    #[test]
    fn 이어진_벽은_mline_하나로_묶인다() {
        // 벽 셋이 ㄴ 자로 이어진다. 묶이지 않으면 코너마다 캡이 선다.
        let j = doc_json(&format!(
            "{},{},{}",
            wall_json(1, 0.0, 0.0, 100.0, 0.0, 20.0),
            wall_json(2, 100.0, 0.0, 0.0, 100.0, 20.0),
            wall_json(3, 100.0, 100.0, 100.0, 0.0, 20.0),
        ));
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.wall_count, 3);
        assert_eq!(r.wall_mline_count, 1, "셋이 한 줄기여야 한다");
    }

    #[test]
    fn 벽의_y_는_뒤집힌다() {
        // 캔버스는 y 가 아래로, 엔진은 위로 간다. 선·글자와 같은 규칙이어야
        // 벽이 도면 위에 제자리로 얹힌다.
        let j = doc_json(&wall_json(1, 10.0, 50.0, 0.0, 30.0, 20.0));
        let r = drawing_from_bimove(&j, "").unwrap();
        let m = r
            .drawing
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::MLine(m) => Some(m),
                _ => None,
            })
            .expect("MLINE");
        let ys: Vec<f64> = m.vertices.iter().map(|v| v.p.y).collect();
        assert_eq!(ys, vec![-50.0, -80.0], "y 를 뒤집어야 한다");
    }

    #[test]
    fn 회전한_벽의_끝점을_돌린다() {
        // rotation 은 캔버스 기준이다. 90도 돌리면 (100,0) 이 (0,100) 이 되고,
        // y 를 뒤집어 -100 으로 간다.
        let j = doc_json(
            r#"{"id":"shape:w1","type":"wall","x":0,"y":0,"rotation":1.5707963267948966,
               "index":"a1","parentId":"page:main",
               "props":{"x2":100,"y2":0,"thickness":20}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        let m = r
            .drawing
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::MLine(m) => Some(m),
                _ => None,
            })
            .expect("MLINE");
        let end = m.vertices[1].p;
        assert!(end.x.abs() < 1e-9, "x: {}", end.x);
        assert!((end.y - -100.0).abs() < 1e-9, "y: {}", end.y);
    }

    #[test]
    fn 못_쓰는_벽은_세서_알린다() {
        // 두께 0 과 길이 0. 조용히 빼면 도형이 사라진 걸 아무도 모른다.
        let j = doc_json(&format!(
            "{},{},{}",
            wall_json(1, 0.0, 0.0, 100.0, 0.0, 0.0),
            wall_json(2, 0.0, 0.0, 0.0, 0.0, 20.0),
            wall_json(3, 0.0, 0.0, 100.0, 0.0, 20.0),
        ));
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.wall_count, 3, "본 것은 셋이다");
        assert_eq!(r.wall_mline_count, 1, "그린 것은 하나다");
        assert!(r.notes.join(" | ").contains("벽체 2개를 못 그렸다"), "{:?}", r.notes);
    }

    #[test]
    fn 벽도_레이어를_만든다() {
        // 레이어를 안 만들면 엔진이 못 찾아 "0" 으로 보고 색이 틀어진다.
        let j = doc_json(
            r#"{"id":"shape:w1","type":"wall","x":0,"y":0,"rotation":0,"index":"a1",
               "parentId":"page:main","props":{"x2":100,"y2":0,"thickness":20},
               "meta":{"dxfLayer":"벽체"}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert!(r.drawing.layers.iter().any(|l| l.name == "벽체"), "레이어가 없다");
    }

    #[test]
    fn 다른_페이지의_벽은_안_읽는다() {
        let j = format!(
            r#"{{"version":1,
             "pages":[{{"id":"page:main","name":"모형","index":"a1"}},
                      {{"id":"page:two","name":"배치1","index":"a2"}}],
             "shapes":[{},
               {{"id":"shape:w9","type":"wall","x":0,"y":0,"rotation":0,"index":"a9",
                 "parentId":"page:two","props":{{"x2":500,"y2":0,"thickness":20}}}}],
             "records":{{}},"skipped":0}}"#,
            wall_json(1, 0.0, 0.0, 100.0, 0.0, 20.0),
        );
        let r = drawing_from_bimove(&j, "page:main").unwrap();
        assert_eq!(r.wall_count, 1);
    }

    // ── 문·창 ──

    /// 문 하나. `x`/`y` 는 **가운데**다 (벽선에 스냅된 그 점).
    fn door_json(i: u32, x: f64, y: f64, w: f64, t: f64) -> String {
        format!(
            r#"{{"id":"shape:d{i}","type":"door","x":{x},"y":{y},"rotation":0,"index":"b{i}",
               "parentId":"page:main","props":{{"width":{w},"thickness":{t},"swing":1,"flipped":false}}}}"#
        )
    }

    fn window_json(i: u32, x: f64, y: f64, w: f64, t: f64) -> String {
        format!(
            r#"{{"id":"shape:n{i}","type":"window","x":{x},"y":{y},"rotation":0,"index":"c{i}",
               "parentId":"page:main","props":{{"width":{w},"thickness":{t}}}}}"#
        )
    }

    fn count_kind(r: &BimoveLoad, f: impl Fn(&EntityKind) -> bool) -> usize {
        r.drawing.model.iter().filter(|e| f(&e.kind)).count()
    }

    #[test]
    fn 문과_창을_엔티티로_넣는다() {
        let j = doc_json(&format!("{},{}", door_json(1, 0.0, 0.0, 80.0, 20.0), window_json(2, 300.0, 0.0, 60.0, 20.0)));
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.opening_count, 2);
        // 문·창 하나마다 가림 하나
        assert_eq!(count_kind(&r, |k| matches!(k, EntityKind::Wipeout(_))), 2);
        // 스윙 호는 문에만
        assert_eq!(count_kind(&r, |k| matches!(k, EntityKind::Arc(_))), 1);
    }

    #[test]
    fn 문은_벽보다_나중에_들어간다() {
        // 차례가 뒤집히면 벽이 문을 덮어서 갭이 사라진다. 그리는 차례에
        // 기대는 구조라 이 순서가 **동작의 일부**다.
        let j = doc_json(&format!("{},{}", door_json(1, 50.0, 0.0, 80.0, 20.0), wall_json(2, 0.0, 0.0, 200.0, 0.0, 20.0)));
        let r = drawing_from_bimove(&j, "").unwrap();
        let pos = |f: fn(&EntityKind) -> bool| r.drawing.model.iter().position(|e| f(&e.kind));
        let mline = pos(|k| matches!(k, EntityKind::MLine(_))).expect("벽이 없다");
        let gap = pos(|k| matches!(k, EntityKind::Wipeout(_))).expect("가림이 없다");
        assert!(mline < gap, "벽 {mline} / 가림 {gap} — 문이 벽보다 먼저 들어갔다");
    }

    #[test]
    fn 문의_갭이_벽_위에_놓인다() {
        // 가운데가 (50, 0) 이면 엔진에서도 y 가 0 이고 x 는 10..90 이다
        let j = doc_json(&door_json(1, 50.0, 0.0, 80.0, 20.0));
        let r = drawing_from_bimove(&j, "").unwrap();
        let b = r
            .drawing
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Wipeout(w) => Some(w.boundary.clone()),
                _ => None,
            })
            .expect("가림이 없다");
        assert_eq!(b.iter().map(|p| p.x).fold(f64::MAX, f64::min), 10.0);
        assert_eq!(b.iter().map(|p| p.x).fold(f64::MIN, f64::max), 90.0);
    }

    #[test]
    fn 문의_y_도_뒤집힌다() {
        let j = doc_json(&door_json(1, 0.0, 60.0, 80.0, 20.0));
        let r = drawing_from_bimove(&j, "").unwrap();
        let ys: Vec<f64> = r
            .drawing
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Wipeout(w) => Some(w.boundary.iter().map(|p| p.y).collect()),
                _ => None,
            })
            .expect("가림이 없다");
        assert!(ys.iter().all(|y| *y < 0.0), "{ys:?} — y 가 안 뒤집혔다");
    }

    #[test]
    fn 못_쓰는_문은_세서_알린다() {
        // 폭 0 짜리. 조용히 빼면 도형이 사라진 걸 아무도 모른다.
        let j = doc_json(&door_json(1, 0.0, 0.0, 0.0, 20.0));
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.opening_count, 1);
        assert_eq!(count_kind(&r, |k| matches!(k, EntityKind::Wipeout(_))), 0);
        assert!(r.notes.iter().any(|n| n.contains("문·창 1개")), "{:?}", r.notes);
    }

    #[test]
    fn 문도_레이어를_만든다() {
        let j = doc_json(
            r#"{"id":"shape:d1","type":"door","x":0,"y":0,"rotation":0,"index":"b1",
                "parentId":"page:main","props":{"width":80,"thickness":20,"swing":1},
                "meta":{"dxfLayer":"A-DOOR"}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert!(r.drawing.layers.iter().any(|l| l.name == "A-DOOR"), "레이어가 없다");
    }

    #[test]
    fn 스윙이_0_이면_한쪽으로_정한다() {
        // 0 을 그대로 쓰면 문짝이 벽 위에 겹쳐 눕는다
        let j = doc_json(
            r#"{"id":"shape:d1","type":"door","x":0,"y":0,"rotation":0,"index":"b1",
                "parentId":"page:main","props":{"width":80,"thickness":20,"swing":0}}"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        let arc = r
            .drawing
            .model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Arc(a) => Some(a.clone()),
                _ => None,
            })
            .expect("호가 없다");
        let span = (arc.end - arc.start).rem_euclid(std::f64::consts::TAU);
        assert!((span - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{}도", span.to_degrees());
    }

    // ── 해치 ──

    fn hatch_doc(hatches: &str) -> String {
        format!(
            r#"{{"version":1,"pages":[{{"id":"page:main","name":"모형","index":"a1"}}],
             "shapes":[{{"id":"shape:g","type":"dxfgroup","x":0,"y":0,"rotation":0,"index":"a1",
               "parentId":"page:main","props":{{"pathData":"","hatchesJson":{}}},
               "meta":{{"dxfLayer":"바닥"}}}}],
             "records":{{}},"skipped":0,
             "source":{{"from":"tldraw","at":0,"sourceHash":"x","shapeCount":1}}}}"#,
            serde_json::Value::String(hatches.to_string())
        )
    }

    #[test]
    fn 해치가_경계와_패턴을_갖고_들어온다() {
        let j = hatch_doc(
            r#"[{"d":"M0,0L100,0L100,100L0,100Z","p":"ANSI31","s":1,"a":45,
                 "dl":[{"a":45,"s":10}],"dim":100}]"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(r.hatch_count, 1);
        match &r.drawing.model.iter().next().unwrap().kind {
            EntityKind::Hatch(h) => {
                assert_eq!(h.pattern, "ANSI31");
                assert!(!h.solid);
                assert_eq!(h.loops.len(), 1);
                // 닫는 점은 되풀이하지 않는다 — 사각형은 꼭짓점 4개다
                assert_eq!(h.loops[0].vertices.len(), 4);
                assert!(h.loops[0].outer, "바깥 고리가 없으면 안 칠해질 수 있다");
                // y 가 뒤집혀 있다
                let ys: Vec<f64> = h.loops[0].vertices.iter().map(|v| v.p.y).collect();
                assert!(ys.iter().all(|y| *y <= 0.0), "{ys:?}");
                // 간격·각도를 우리 공간으로 옮겨 실었으니 여기서 또 돌리면 안 된다
                assert_eq!(h.scale, 1.0);
                assert_eq!(h.angle, 0.0);
                assert_eq!(h.pattern_lines.len(), 1);
                assert_eq!(h.pattern_lines[0].angle, -45.0, "y 뒤집힘이 각도에 반영돼야 한다");
                assert_eq!(h.pattern_lines[0].delta, (0.0, 10.0));
            }
            k => panic!("Hatch 가 아니다: {}", k.type_name()),
        }
    }

    #[test]
    fn 솔리드는_패턴을_안_만든다() {
        let j = hatch_doc(r#"[{"d":"M0,0L10,0L10,10Z","p":"SOLID","f":1,"dim":10}]"#);
        let r = drawing_from_bimove(&j, "").unwrap();
        match &r.drawing.model.iter().next().unwrap().kind {
            EntityKind::Hatch(h) => {
                assert!(h.solid);
                assert!(h.pattern_lines.is_empty());
            }
            k => panic!("Hatch 가 아니다: {}", k.type_name()),
        }
    }

    #[test]
    fn 구멍이_있는_해치는_고리가_여럿이다() {
        // 안쪽 고리를 잃으면 구멍이 메워져 나온다.
        let j = hatch_doc(
            r#"[{"d":"M0,0L100,0L100,100L0,100ZM30,30L70,30L70,70L30,70Z","p":"ANSI31",
                 "dl":[{"a":45,"s":10}],"dim":100,"hs":1}]"#,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        match &r.drawing.model.iter().next().unwrap().kind {
            EntityKind::Hatch(h) => {
                assert_eq!(h.loops.len(), 2);
                // 넓은 쪽이 바깥이다
                assert!(h.loops[0].outer && !h.loops[1].outer);
                assert_eq!(h.style, 1);
            }
            k => panic!("Hatch 가 아니다: {}", k.type_name()),
        }
    }

    #[test]
    fn 간격을_해치_크기에_가둔다() {
        // 아주 촘촘한 간격은 통짜 회색으로 뭉개지고 엔진이 선을 수만 개 만든다.
        assert_eq!(clamp_spacing(0.001, 600.0), 10.0);
        // 너무 성기면 해치 안에 선이 한 줄도 안 들어간다
        assert_eq!(clamp_spacing(99999.0, 600.0), 150.0);
        // 멀쩡한 값은 그대로
        assert_eq!(clamp_spacing(20.0, 600.0), 20.0);
        // dim 이 말이 안 되면 기본값으로 민다
        assert!(clamp_spacing(1.0, f64::NAN) > 1.0);
    }

    #[test]
    fn 패턴_해치는_정의선을_반드시_하나는_갖는다() {
        // 비어 있으면 엔진이 표준 라이브러리 패턴을 **우리 scale(=1)** 로
        // 그린다. 라이브러리 ANSI31 의 간격은 0.125 도면 단위인데 우리 단위는
        // 캔버스 px 라, 0.125px 간격이 되어 해치가 통짜로 뭉개진다.
        for h in [
            r#"[{"d":"M0,0L100,0L100,100L0,100Z","p":"ANSI31","dim":100}]"#,
            r#"[{"d":"M0,0L100,0L100,100L0,100Z","p":"모르는패턴","dim":100}]"#,
            r#"[{"d":"M0,0L100,0L100,100L0,100Z","dim":100}]"#,
            // 간격이 0 이거나 말이 안 되면 그 줄은 버려지는데, 그래도 비면 안 된다
            r#"[{"d":"M0,0L100,0L100,100L0,100Z","p":"ANSI31","dl":[{"a":0,"s":0}],"dim":100}]"#,
        ] {
            let r = drawing_from_bimove(&hatch_doc(h), "").unwrap();
            match &r.drawing.model.iter().next().unwrap().kind {
                EntityKind::Hatch(e) => {
                    assert!(!e.solid);
                    assert!(!e.pattern_lines.is_empty(), "{h}");
                    let sp = e.pattern_lines[0].delta.1;
                    assert!(sp >= 100.0 / 60.0 && sp <= 100.0 / 4.0, "간격 {sp} 가 울타리 밖이다");
                }
                k => panic!("Hatch 가 아니다: {}", k.type_name()),
            }
        }
    }

    #[test]
    fn 색이_있으면_해치가_그_색이다() {
        let j = hatch_doc(
            r##"[{"d":"M0,0L10,0L10,10Z","p":"ANSI31","c":"#0000ff","dl":[{"a":0,"s":2}],"dim":10}]"##,
        );
        let r = drawing_from_bimove(&j, "").unwrap();
        assert_eq!(
            r.drawing.model.iter().next().unwrap().common.color,
            Color::True(Rgb(0, 0, 0xff))
        );
    }

    #[test]
    fn 말이_안_되는_해치는_건너뛴다() {
        // 점이 셋도 안 되면 칠할 넓이가 없다. 터지지 말고 넘어가야 한다.
        for bad in [
            r#"[{"d":"M0,0L10,0","p":"ANSI31"}]"#,
            r#"[{"p":"ANSI31"}]"#,
            r#"[]"#,
            r#"그냥 글자"#,
        ] {
            let r = drawing_from_bimove(&hatch_doc(bad), "").unwrap();
            assert_eq!(r.hatch_count, 0, "{bad}");
        }
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
