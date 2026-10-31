//! Bimove 물량 산출표를 엔진 TABLE 로 옮긴다.
//!
//! ── 왜 TABLE 인가 ──
//!
//! 이걸 선·글자로 풀어 넣을 수도 있다 (화면이 SVG 로 그러고 있다). 그러면
//! 그림은 같지만 **표가 아니게 된다** — 칸을 집을 수도, 고칠 수도, DXF 로
//! 내보냈을 때 다른 CAD 에서 표로 읽을 수도 없다. 도면에 얹히는 집계표는
//! ACAD_TABLE 이 제자리다.
//!
//! 엔진의 `EntityKind::Table` 은 칸 너비·줄 높이·병합·셀 스타일(채움·정렬·
//! 여백·테두리별 가시성)을 다 들고 있고, 렌더러가 그걸로 격자와 글자를
//! 만든다. 그래서 화면의 모양을 **스타일로** 적어 넣으면 된다.
//!
//! ── 파생 뷰가 아니다 ──
//!
//! 한때 "물량표는 BOQ 데이터에서 매번 다시 만들어지는 것이라 엔진에 굳히면
//! 안 된다" 고 생각했다. 그게 틀렸다 — `BoqTableShape` 의 머리글이 이렇게
//! 적어 두었다: *"값은 만들 때 한 번 받아 props 에 담는다(스냅샷). 저장된
//! 도면을 나중에 열거나 공유 링크로 열어도 그때 본 숫자가 그대로 남아야
//! 하고, 그게 출력물의 성격에도 맞다."* 자재를 고친 뒤 **더블클릭으로 다시
//! 읽는** 것이지, 저절로 따라오는 값이 아니다. 즉 도면 데이터다.
//!
//! ── 합계줄의 세로 괘선 ──
//!
//! 화면은 세로 괘선을 **합계줄 바로 위까지만** 내린다. 끝까지 내리면 합계
//! 금액 글자를 세로줄이 관통한다. 엔진에서는 그걸 셀 스타일로 적는다:
//! 서로 다른 스타일이 맞닿은 자리의 괘선은 **양쪽이 다 보이라고 해야** 그려
//! 지므로, 합계줄 스타일들의 좌우 테두리를 끄면 그 줄만 괘선이 사라진다.
//! 표 바깥 테두리는 맨 왼쪽·오른쪽 스타일에만 남겨 둔다.
//!
//! ── 글자는 잘라서 넣는다 ──
//!
//! 칸 너비가 고정이라 긴 글자는 옆 칸을 침범한다. 화면은 글자 수로 어림해
//! 잘라 `…` 를 붙인다 (`fitText`). 엔진의 MTEXT 는 칸을 넘어도 안 잘리므로,
//! **여기서 같은 규칙으로 미리 잘라야** 화면과 같은 그림이 된다.

use cadcraft_color::{Color, Rgb};
use cadcraft_doc::{
    CellBorder, CellBorders, CellStyle, Drawing, Entity, EntityKind, Table, TableCell, TableStyle,
};
use cadcraft_geom::{Vec2, Vec3};

use crate::jsnum::to_fixed;

/// 표 한 줄. 화면의 `BoqRow` 와 같은 모양이다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoqRow {
    pub category: String,
    pub item: String,
    pub spec: String,
    /// 면적 (m²). 길이로 세는 자재는 0.
    pub area_m2: f64,
    pub qty: f64,
    pub unit: String,
}

/// 물량표 하나. 좌표는 **캔버스** 것이다 — y 뒤집기는 이 안에서 한다.
#[derive(Clone, Debug, PartialEq)]
pub struct BoqTable {
    /// 이 물량표를 만든 Bimove 도형의 id (`crate::identity`).
    pub id: String,
    /// 캔버스에서의 **좌상단** (도형의 `x,y`).
    pub at: Vec2,
    pub title: String,
    pub rows: Vec<BoqRow>,
    pub total_area_m2: f64,
    pub layer: String,
}

/// 칸 하나. 화면의 `BOQ_COLUMNS` 와 같은 너비·정렬이다.
struct Column {
    label: &'static str,
    width: f64,
    align: Align,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Align {
    Left,
    Right,
    Center,
}

impl Align {
    /// MTEXT 붙임점 — 세로는 늘 가운데다 (화면도 `y + ROW_H/2` 에 찍는다).
    pub(crate) fn attach(self) -> u8 {
        match self {
            Align::Left => 4,
            Align::Center => 5,
            Align::Right => 6,
        }
    }
    pub(crate) fn suffix(self) -> &'static str {
        match self {
            Align::Left => "L",
            Align::Center => "C",
            Align::Right => "R",
        }
    }
}

const COLUMNS: [Column; 6] = [
    Column { label: "구분", width: 88.0, align: Align::Left },
    Column { label: "품명", width: 104.0, align: Align::Left },
    Column { label: "규격", width: 228.0, align: Align::Left },
    Column { label: "면적(m²)", width: 78.0, align: Align::Right },
    Column { label: "수량", width: 66.0, align: Align::Right },
    Column { label: "단위", width: 48.0, align: Align::Center },
];

/// 화면의 `BOQ_TITLE_H` / `BOQ_HEADER_H` / `BOQ_ROW_H`.
pub(crate) const TITLE_H: f64 = 28.0;
pub(crate) const HEADER_H: f64 = 24.0;
pub(crate) const ROW_H: f64 = 22.0;

/// 화면의 `FONT` / `TITLE_FONT`.
pub(crate) const FONT: f64 = 11.0;
pub(crate) const TITLE_FONT: f64 = 13.0;

/// 화면의 `INK` / `RULE`, 그리고 "내용 없음" 글자색.
pub(crate) const INK: Rgb = Rgb(0x33, 0x33, 0x33);
pub(crate) const RULE: Rgb = Rgb(0x9a, 0xa0, 0xa6);
pub(crate) const MUTED: Rgb = Rgb(0x99, 0x99, 0x99);

/// 채움색 — 화면의 제목줄·머리줄·합계줄.
pub(crate) const TITLE_FILL: Rgb = Rgb(0xf1, 0xf3, 0xf4);
pub(crate) const HEADER_FILL: Rgb = Rgb(0xfa, 0xfa, 0xfa);
pub(crate) const TOTAL_FILL: Rgb = Rgb(0xf6, 0xf8, 0xfa);

/// 칸 안쪽 여백. 화면의 `cellTextX` 기본 `pad = 6`, 제목·합계는 `x = 8`.
pub(crate) const PAD: f64 = 6.0;
pub(crate) const EDGE_PAD: f64 = 8.0;

/// 표 스타일 이름. 표가 여럿이어도 모양은 하나라 스타일도 하나면 된다.
pub const STYLE: &str = "BIMOVE_BOQ";

fn table_width() -> f64 {
    COLUMNS.iter().map(|c| c.width).sum()
}

/// 글자를 칸 안에 들어가게 자르고 `…` 를 붙인다.
///
/// 화면의 `fitText` 와 **같은 어림**이다 — 글꼴을 재지 않고 글자 수로 센다.
/// 한글·한자·가나를 1 로, 그 밖을 0.55 로 본다. 엔진에서 제대로 재면 더
/// 정확하겠지만, 그러면 화면과 다른 자리에서 잘려서 두 그림이 달라진다.
pub(crate) fn fit_text(text: &str, width: f64) -> String {
    let budget = (width - PAD * 2.0) / FONT;
    // 화면의 `/[ㄱ-힝一-鿿぀-ヿ]/` 과 **같은 집합**이다.
    //
    // 한자 범위는 한글 범위 안에 통째로 들어간다 (U+4E00..U+9FFF ⊂
    // U+3131..U+D79D) — 그대로 옮겨 적으면 "닿지 않는 갈래" 경고가 난다.
    // 그렇다고 둘을 `U+3040..U+D79D` 한 줄로 합치면 **안 된다**: 가나 끝과
    // 한글 시작 사이의 U+3100..U+3130 (31자) 이 딸려 들어와 화면과 다른
    // 자리에서 잘린다.
    let wide = |c: char| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{3131}'..='\u{D79D}');
    let mut used = 0.0;
    for (i, c) in text.chars().enumerate() {
        used += if wide(c) { 1.0 } else { 0.55 };
        if used > budget {
            // '…' 자리를 남기려고 한 글자 더 물러선다 (화면과 같다)
            let keep = if i > 0 { i - 1 } else { 0 };
            return text.chars().take(keep).collect::<String>() + "…";
        }
    }
    text.to_string()
}

/// 한 칸에 찍을 글자. 숫자 칸은 화면과 같은 자릿수로 맞춘다.
fn cell_text(row: &BoqRow, col: usize) -> String {
    match col {
        0 => row.category.clone(),
        1 => row.item.clone(),
        2 => row.spec.clone(),
        3 => to_fixed(row.area_m2, 2),
        // 화면: `Number.isInteger(n) ? String(n) : n.toFixed(2)`
        4 => {
            if row.qty.fract() == 0.0 && row.qty.is_finite() {
                format!("{}", row.qty as i64)
            } else {
                to_fixed(row.qty, 2)
            }
        }
        _ => row.unit.clone(),
    }
}

/// 테두리 하나.
pub(crate) fn border(visible: bool) -> CellBorder {
    // 색과 굵기는 `ByBlock` 으로 둔다 — 엔티티의 색(괘선색)을 따라간다.
    CellBorder { visible, ..CellBorder::default() }
}

/// 사방 테두리. `left`/`right` 만 따로 받는다 — 합계줄에서 그 둘을 끈다.
pub(crate) fn borders(left: bool, right: bool) -> CellBorders {
    CellBorders {
        left: border(left),
        right: border(right),
        inside_v: border(left && right),
        ..CellBorders::default()
    }
}

pub(crate) fn style(
    name: &str, height: f64, align: Align, fill: Option<Rgb>, ink: Rgb, pad: f64,
    sides: (bool, bool),
) -> CellStyle {
    CellStyle {
        name: name.into(),
        text_style: "Standard".into(),
        text_height: height,
        text_color: Color::True(ink),
        fill: fill.map(Color::True),
        align: align.attach(),
        margin_h: pad,
        margin_v: 0.0,
        borders: borders(sides.0, sides.1),
        ..CellStyle::default()
    }
}

/// 셀 스타일 이름들 — 줄 종류 + 정렬.
fn head_style(align: Align) -> String {
    format!("{STYLE}_HEAD_{}", align.suffix())
}
fn data_style(align: Align) -> String {
    format!("{STYLE}_DATA_{}", align.suffix())
}
const TITLE_STYLE: &str = "BIMOVE_BOQ_TITLE";
const EMPTY_STYLE: &str = "BIMOVE_BOQ_EMPTY";
/// 합계줄 — 왼쪽(라벨), 가운데(금액), 오른쪽(빈 칸). 좌우 괘선을 끄려고 셋이다.
const TOTAL_L: &str = "BIMOVE_BOQ_TOTAL_L";
const TOTAL_M: &str = "BIMOVE_BOQ_TOTAL_M";
const TOTAL_R: &str = "BIMOVE_BOQ_TOTAL_R";

/// 표 스타일을 도면에 넣는다. 같은 이름이 있으면 **갈아끼운다.**
fn register(drawing: &mut Drawing) {
    // 표의 `text_height` 와 같게 둔다. 다르면 렌더러가 그 비로 셀 스타일을
    // 통째로 키워서(`style_scale`) 글자 크기가 두 번 먹는다.
    let mut st = TableStyle {
        name: STYLE.into(),
        text_height: FONT,
        margin: PAD,
        title: true,
        header: true,
        ..TableStyle::default()
    };

    st.set_cell_style(style(TITLE_STYLE, TITLE_FONT, Align::Left, Some(TITLE_FILL), INK, EDGE_PAD, (true, true)));
    st.set_cell_style(style(EMPTY_STYLE, FONT, Align::Center, None, MUTED, PAD, (true, true)));
    for align in [Align::Left, Align::Center, Align::Right] {
        st.set_cell_style(style(&head_style(align), FONT, align, Some(HEADER_FILL), INK, PAD, (true, true)));
        st.set_cell_style(style(&data_style(align), FONT, align, None, INK, PAD, (true, true)));
    }
    // 합계줄은 칸을 쓰지 않고 한 줄을 가로질러 적는다 — 세로 괘선을 끈다
    st.set_cell_style(style(TOTAL_L, FONT, Align::Left, Some(TOTAL_FILL), INK, EDGE_PAD, (true, false)));
    st.set_cell_style(style(TOTAL_M, FONT, Align::Right, Some(TOTAL_FILL), INK, PAD, (false, false)));
    st.set_cell_style(style(TOTAL_R, FONT, Align::Left, Some(TOTAL_FILL), INK, PAD, (false, true)));

    match drawing.table_styles.iter_mut().find(|s| s.name == st.name) {
        Some(old) => *old = st,
        None => drawing.table_styles.push(st),
    }
}

pub(crate) fn cell(text: impl Into<String>, style: &str) -> TableCell {
    TableCell { text: text.into(), style: style.into(), ..TableCell::default() }
}

pub(crate) fn merged(text: impl Into<String>, style: &str, cols: u32) -> TableCell {
    TableCell { merged: Some((1, cols)), ..cell(text, style) }
}

/// 빈 표도 줄 하나는 그린다 — 화면과 같다. 선 하나로 쪼그라들면 도면에서
/// 못 찾는다.
fn body_rows(t: &BoqTable) -> Vec<Vec<TableCell>> {
    if t.rows.is_empty() {
        return vec![vec![merged("물량이 입력된 마감재가 없습니다", EMPTY_STYLE, COLUMNS.len() as u32)]];
    }
    t.rows
        .iter()
        .map(|row| {
            COLUMNS
                .iter()
                .enumerate()
                .map(|(i, c)| cell(fit_text(&cell_text(row, i), c.width), &data_style(c.align)))
                .collect()
        })
        .collect()
}

/// 합계줄 — 라벨 왼쪽, 금액은 **면적 칸의 오른쪽 끝**에 맞춘다 (화면의
/// `xs[4] - 6`). 나머지 두 칸은 비워 둔다.
///
/// **줄은 칸 수만큼 길어야 한다.** `cells[row][i]` 의 `i` 는 **열 번호**다 —
/// 병합에 먹힌 칸도 자리를 차지한다. 짧게 넣으면 뒤 칸이 앞으로 밀려서
/// 병합 범위에 삼켜지고, 그 칸은 **아예 안 그려진다** (합계 금액이 그렇게
/// 사라졌다). 먹힌 자리는 빈 칸으로 채운다.
fn total_row(t: &BoqTable) -> Vec<TableCell> {
    let mut row = vec![TableCell::default(); COLUMNS.len()];
    row[0] = merged("합계", TOTAL_L, 3);
    // 1·2 는 위 병합에 먹힌다
    row[3] = cell(format!("{} m²", to_fixed(t.total_area_m2, 2)), TOTAL_M);
    row[4] = merged("", TOTAL_R, 2);
    // 5 는 그 병합에 먹힌다
    row
}

/// 쓸 수 있는 표인가. 자리가 NaN 이면 어디에 그릴지 알 수 없다.
pub fn usable(t: &BoqTable) -> bool {
    t.at.is_finite()
}

fn entity_for(t: &BoqTable) -> Table {
    let mut cells: Vec<Vec<TableCell>> =
        vec![vec![merged(fit_text(&t.title, table_width()), TITLE_STYLE, COLUMNS.len() as u32)]];
    cells.push(
        COLUMNS.iter().map(|c| cell(c.label, &head_style(c.align))).collect(),
    );
    let body = body_rows(t);
    let mut heights = vec![TITLE_H, HEADER_H];
    heights.extend(body.iter().map(|_| ROW_H));
    heights.push(ROW_H);
    cells.extend(body);
    cells.push(total_row(t));

    Table {
        // 표는 넣은 점에서 **아래로** 자란다 (렌더러가 `y` 를 빼며 내려간다).
        // 캔버스의 좌상단이 그대로 여기다 — y 만 뒤집는다.
        insert: Vec3::new(t.at.x, -t.at.y, 0.0),
        col_widths: COLUMNS.iter().map(|c| c.width).collect(),
        row_heights: heights,
        cells,
        style: STYLE.into(),
        // 스타일의 높이와 같게 — 다르면 셀 스타일이 통째로 그 비로 커진다
        text_height: FONT,
        title: true,
        header: true,
        row_styles: Vec::new(),
        breaks: None,
        data_link: None,
    }
}

/// 물량표를 도면에 넣는다.
///
/// @returns 넣은 개수. 못 쓰는 것은 세지 않는다 — 부르는 쪽이 그 차이를 알려야 한다.
pub fn add_tables(drawing: &mut Drawing, tables: &[BoqTable]) -> usize {
    if !tables.iter().any(usable) {
        return 0;
    }
    register(drawing);

    let mut n = 0;
    for t in tables.iter().filter(|t| usable(t)) {
        let h = drawing.new_handle();
        let mut e = Entity::new(h, EntityKind::Table(entity_for(t)));
        e.common.layer = t.layer.clone();
        // 괘선색이다. 셀 스타일의 테두리는 `ByBlock` 이라 이걸 따라간다.
        e.common.color = Color::True(RULE);
        let from = drawing.model.len();
        drawing.model.push(e);
        crate::identity::stamp_one(drawing, from, crate::identity::BOQ, &t.id);
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cat: &str, item: &str, spec: &str, area: f64, qty: f64, unit: &str) -> BoqRow {
        BoqRow {
            category: cat.into(),
            item: item.into(),
            spec: spec.into(),
            area_m2: area,
            qty,
            unit: unit.into(),
        }
    }

    fn table(rows: Vec<BoqRow>) -> BoqTable {
        BoqTable {
            id: String::new(),
            at: Vec2::new(100.0, 50.0),
            title: "마감재 물량 산출표".into(),
            total_area_m2: 12.5,
            rows,
            layer: "A-BOQ".into(),
        }
    }

    fn only(d: &Drawing) -> Table {
        d.model
            .iter()
            .find_map(|e| match &e.kind {
                EntityKind::Table(t) => Some(t.clone()),
                _ => None,
            })
            .expect("표가 없다")
    }

    #[test]
    fn 표를_하나_넣는다() {
        let mut d = Drawing::new_metric();
        assert_eq!(add_tables(&mut d, &[table(vec![row("벽체", "석고", "9.5T", 1.0, 2.0, "장")])]), 1);
        let t = only(&d);
        assert_eq!(t.style, STYLE);
        assert!(t.title && t.header);
        assert_eq!(t.col_widths.len(), 6);
    }

    #[test]
    fn 줄_높이가_화면과_같다() {
        // 제목 + 머리 + 본문 + 합계
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "나", "다", 1.0, 1.0, "개"); 3])]);
        let t = only(&d);
        assert_eq!(t.row_heights, vec![28.0, 24.0, 22.0, 22.0, 22.0, 22.0]);
        // 총 높이가 `boqTableSize` 와 같아야 한다
        let total: f64 = t.row_heights.iter().sum();
        assert_eq!(total, 28.0 + 24.0 + 3.0 * 22.0 + 22.0);
    }

    #[test]
    fn 칸_너비가_화면과_같다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        assert_eq!(only(&d).col_widths, vec![88.0, 104.0, 228.0, 78.0, 66.0, 48.0]);
        assert_eq!(table_width(), 612.0);
    }

    #[test]
    fn 좌상단에서_아래로_자란다() {
        // 렌더러가 넣은 점에서 y 를 빼며 내려가므로, 캔버스 좌상단이 그대로 들어간다
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        assert_eq!(only(&d).insert.xy(), Vec2::new(100.0, -50.0));
    }

    #[test]
    fn 제목은_한_줄을_가로지른다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let t = only(&d);
        assert_eq!(t.cells[0].len(), 1, "제목 줄이 쪼개져 있다");
        assert_eq!(t.cells[0][0].merged, Some((1, 6)));
        assert_eq!(t.cells[0][0].text, "마감재 물량 산출표");
        assert_eq!(t.cells[0][0].style, TITLE_STYLE);
    }

    #[test]
    fn 머리줄이_칸_이름을_들고_있다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let t = only(&d);
        let head: Vec<&str> = t.cells[1].iter().map(|c| c.text.as_str()).collect();
        assert_eq!(head, vec!["구분", "품명", "규격", "면적(m²)", "수량", "단위"]);
    }

    #[test]
    fn 숫자_칸의_자릿수가_화면과_같다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "나", "다", 8.125, 3.0, "개")])]);
        let body = &only(&d).cells[2];
        // 면적은 늘 소수 둘. 8.125 는 딱 중간이라 JS 가 "8.13" 을 낸다
        assert_eq!(body[3].text, "8.13");
        // 수량은 정수면 소수점 없이
        assert_eq!(body[4].text, "3");
    }

    #[test]
    fn 수량이_정수가_아니면_소수_둘이다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "나", "다", 1.0, 2.5, "개")])]);
        assert_eq!(only(&d).cells[2][4].text, "2.50");
    }

    #[test]
    fn 긴_글자를_잘라_말줌임표를_붙인다() {
        // 칸 너비가 고정이라 안 자르면 옆 칸을 침범한다
        let long = "아주아주아주아주아주아주아주아주긴규격이름입니다";
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "나", long, 1.0, 1.0, "개")])]);
        let spec = &only(&d).cells[2][2].text;
        assert!(spec.ends_with('…'), "{spec}");
        assert!(spec.chars().count() < long.chars().count(), "{spec}");
    }

    #[test]
    fn 짧은_글자는_안_건드린다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("벽체 마감", "석고보드", "9.5T 일반", 1.0, 1.0, "장")])]);
        let body = &only(&d).cells[2];
        assert_eq!(body[0].text, "벽체 마감");
        assert_eq!(body[2].text, "9.5T 일반");
    }

    #[test]
    fn 자르는_자리가_화면과_같다() {
        // 화면의 `fitText` 와 같은 어림이어야 한다 — 한글 1, 그 밖 0.55.
        // 규격 칸(228px, 글꼴 11) 의 예산은 (228-12)/11 = 19.64 글자다.
        assert_eq!(fit_text("가나다라마바사아자차카타파하가나다라마", 228.0).chars().count(), 19);
        // 영문은 0.55 로 세므로 더 많이 들어간다
        assert!(fit_text(&"a".repeat(30), 228.0).chars().count() > 19);
        // 빈 글자와 딱 맞는 글자는 그대로
        assert_eq!(fit_text("", 228.0), "");
        assert_eq!(fit_text("가나다", 228.0), "가나다");
    }

    #[test]
    fn 빈_표도_한_줄을_그린다() {
        // 선 하나로 쪼그라들면 도면에서 못 찾는다
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let t = only(&d);
        assert_eq!(t.row_heights.len(), 4, "제목+머리+내용없음+합계");
        assert_eq!(t.cells[2].len(), 1);
        assert_eq!(t.cells[2][0].text, "물량이 입력된 마감재가 없습니다");
        assert_eq!(t.cells[2][0].merged, Some((1, 6)));
    }

    #[test]
    fn 합계줄의_금액이_면적_칸에_있다() {
        // `cells[row][i]` 의 `i` 는 **열 번호**다. 짧게 넣으면 금액이 1열로
        // 밀려 "합계" 의 병합(0..2)에 삼켜지고 아예 안 그려진다.
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let t = only(&d);
        let last = t.cells.last().expect("합계줄");
        assert_eq!(last.len(), COLUMNS.len(), "줄이 칸 수만큼 길어야 한다");
        assert_eq!(last[0].text, "합계");
        assert_eq!(last[0].merged, Some((1, 3)));
        // 면적 칸(3열) — 화면도 그 칸의 오른쪽 끝에 맞춘다
        assert_eq!(last[3].text, "12.50 m²");
        assert_eq!(last[3].style, TOTAL_M);
        assert_eq!(last[4].merged, Some((1, 2)));
        // 병합에 먹힌 자리는 비어 있다
        assert!(last[1].text.is_empty() && last[2].text.is_empty() && last[5].text.is_empty());
    }

    #[test]
    fn 그려진_합계줄에_세로_괘선이_없다() {
        // 데이터만 봐서는 못 잡는다 — 셀을 엉뚱한 열에 넣으면 거기가 그냥
        // 본문 스타일이 되어 괘선이 되살아난다. 그려진 선을 직접 본다.
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "나", "다", 1.0, 1.0, "개")])]);
        let t = only(&d);
        let g = cadcraft_render::table_geometry(&d, &t);

        let top: f64 = t.row_heights.iter().rev().skip(1).sum();
        let (y0, y1) = (t.insert.y - top, t.insert.y - top - ROW_H);
        let mid = (y0 + y1) / 2.0;
        let (left, right) = (t.insert.x, t.insert.x + table_width());

        let inside: Vec<f64> = g
            .lines
            .iter()
            // 세로선이고, 합계줄 높이를 지나고, 표 안쪽인 것
            .filter(|l| (l.a.x - l.b.x).abs() < 1e-9)
            .filter(|l| l.a.y.min(l.b.y) < mid && l.a.y.max(l.b.y) > mid)
            .filter(|l| l.a.x > left + 1e-9 && l.a.x < right - 1e-9)
            .map(|l| l.a.x - t.insert.x)
            .collect();
        assert!(inside.is_empty(), "합계줄을 가로지르는 세로 괘선 {inside:?}");

        // 금액이 실제로 그려지는지도 같이 본다
        assert!(
            g.texts.iter().any(|x| x.text.contains("m²")),
            "합계 금액이 안 그려진다: {:?}",
            g.texts.iter().map(|x| &x.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn 그려진_본문줄에는_세로_괘선이_있다() {
        // 위 테스트가 "전부 꺼짐" 으로도 통과하면 안 된다
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "나", "다", 1.0, 1.0, "개")])]);
        let t = only(&d);
        let g = cadcraft_render::table_geometry(&d, &t);

        let top = TITLE_H + HEADER_H;
        let mid = t.insert.y - top - ROW_H / 2.0;
        let (left, right) = (t.insert.x, t.insert.x + table_width());
        let n = g
            .lines
            .iter()
            .filter(|l| (l.a.x - l.b.x).abs() < 1e-9)
            .filter(|l| l.a.y.min(l.b.y) < mid && l.a.y.max(l.b.y) > mid)
            .filter(|l| l.a.x > left + 1e-9 && l.a.x < right - 1e-9)
            .count();
        assert_eq!(n, COLUMNS.len() - 1, "본문 괘선이 {n}개다");
    }

    #[test]
    fn 합계줄의_세로_괘선이_꺼져_있다() {
        // 맞닿은 두 스타일이 **둘 다** 보이라고 해야 괘선이 그려진다.
        // 그래서 안쪽을 끄고 표 바깥 테두리만 남긴다.
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let st = d.table_styles.iter().find(|s| s.name == STYLE).expect("스타일");
        let l = st.cell_style(TOTAL_L);
        let m = st.cell_style(TOTAL_M);
        let r = st.cell_style(TOTAL_R);
        assert!(!(l.borders.right.visible && m.borders.left.visible), "합계 1|2 괘선이 남았다");
        assert!(!(m.borders.right.visible && r.borders.left.visible), "합계 2|3 괘선이 남았다");
        // 표 바깥은 남아 있어야 한다
        assert!(l.borders.left.visible, "표 왼쪽 테두리가 없다");
        assert!(r.borders.right.visible, "표 오른쪽 테두리가 없다");
    }

    #[test]
    fn 본문_칸의_세로_괘선은_그려진다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "나", "다", 1.0, 1.0, "개")])]);
        let st = d.table_styles.iter().find(|s| s.name == STYLE).expect("스타일");
        // 같은 스타일끼리는 `inside_v`, 다른 스타일끼리는 양쪽 테두리
        let l = st.cell_style(&data_style(Align::Left));
        let r = st.cell_style(&data_style(Align::Right));
        assert!(l.borders.inside_v.visible, "구분|품명 괘선이 없다");
        assert!(l.borders.right.visible && r.borders.left.visible, "규격|면적 괘선이 없다");
    }

    #[test]
    fn 정렬이_화면과_같다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let st = d.table_styles.iter().find(|s| s.name == STYLE).expect("스타일");
        // 4 = 가운데-왼쪽, 5 = 가운데-가운데, 6 = 가운데-오른쪽
        assert_eq!(st.cell_style(&data_style(Align::Left)).align, 4);
        assert_eq!(st.cell_style(&data_style(Align::Center)).align, 5);
        assert_eq!(st.cell_style(&data_style(Align::Right)).align, 6);
        assert_eq!(st.cell_style(TITLE_STYLE).align, 4, "제목은 화면에서 왼쪽이다");
    }

    #[test]
    fn 글자_크기가_두_번_먹지_않는다() {
        // 표의 `text_height` 와 스타일의 것이 다르면 렌더러가 그 비로 셀
        // 스타일을 통째로 키운다 (`style_scale`).
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let st = d.table_styles.iter().find(|s| s.name == STYLE).expect("스타일");
        assert_eq!(only(&d).text_height, st.text_height);
        assert_eq!(st.text_height, FONT);
        assert_eq!(st.cell_style(TITLE_STYLE).text_height, TITLE_FONT);
    }

    #[test]
    fn 채움색이_화면과_같다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let st = d.table_styles.iter().find(|s| s.name == STYLE).expect("스타일");
        assert_eq!(st.cell_style(TITLE_STYLE).fill, Some(Color::True(TITLE_FILL)));
        assert_eq!(st.cell_style(&head_style(Align::Left)).fill, Some(Color::True(HEADER_FILL)));
        assert_eq!(st.cell_style(TOTAL_L).fill, Some(Color::True(TOTAL_FILL)));
        // 본문은 안 칠한다
        assert_eq!(st.cell_style(&data_style(Align::Left)).fill, None);
    }

    #[test]
    fn 스타일을_갈아끼운다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        add_tables(&mut d, &[table(vec![])]);
        assert_eq!(d.table_styles.iter().filter(|s| s.name == STYLE).count(), 1);
    }

    #[test]
    fn 레이어가_붙고_색은_괘선색이다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        let e = d.model.iter().next().expect("엔티티");
        assert_eq!(e.common.layer, "A-BOQ");
        // 셀 테두리는 ByBlock 이라 이 색을 따라간다
        assert_eq!(e.common.color, Color::True(RULE));
    }

    #[test]
    fn nan_자리는_안_넣는다() {
        let mut t = table(vec![]);
        t.at = Vec2::new(f64::NAN, 0.0);
        let mut d = Drawing::new_metric();
        assert_eq!(add_tables(&mut d, &[t]), 0);
        assert!(d.model.is_empty());
        // 하나도 안 넣었으면 스타일도 만들지 않는다
        assert!(!d.table_styles.iter().any(|s| s.name == STYLE));
    }

    #[test]
    fn 여러_개를_넣는다() {
        let mut d = Drawing::new_metric();
        let mut b = table(vec![]);
        b.at = Vec2::new(800.0, 50.0);
        assert_eq!(add_tables(&mut d, &[table(vec![]), b]), 2);
        assert_eq!(d.model.iter().filter(|e| matches!(e.kind, EntityKind::Table(_))).count(), 2);
    }

    #[test]
    fn 물량표에_정체성이_붙는다() {
        let mut d = Drawing::new_metric();
        let mut t = table(vec![]);
        t.id = "shape:boq1".into();
        add_tables(&mut d, &[t]);

        let g = crate::identity::of(d.model.iter().next().expect("표가 없다"))
            .expect("표에 정체성이 없다");
        assert_eq!(g.kind, crate::identity::BOQ);
        assert_eq!(g.ids, vec!["shape:boq1"]);
    }

    #[test]
    fn 출처_없는_물량표에는_안_붙인다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![])]);
        assert!(d.model.iter().all(|e| crate::identity::of(e).is_none()));
    }
}
