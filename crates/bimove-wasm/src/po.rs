//! Bimove 발주서를 엔진 TABLE 로 옮긴다.
//!
//! 한: "우선 이 발주서를 도면 쪽에 넣을 수 있는게 있어야지. 도면쪽에 같이
//! 볼 수 있게. 뭔가 내보내기 말고 버튼 누르면 도면에 같이 넣게"
//!
//! ── 왜 `boq.rs` 옆에 따로 두나 ──
//!
//! 그림은 닮았다 — 제목·머리·본문·합계에 같은 글꼴·같은 색이고, 폭도 612 로
//! 맞춰 뒀다 (도면틀 옆에 둘을 나란히 세우면 변이 맞는다). 그래서 칸·테두리·
//! 글자 자르기 같은 잔일은 `boq` 것을 그대로 쓴다.
//!
//! 다른 건 **내용**이다. 물량표는 자재 한 줄씩의 평평한 집계표이고, 발주서는
//! 표(석고보드·타일…) 밑에 품목이 달린 두 층짜리다. 합계줄도 하나가 아니라
//! 표마다 소계가 있고 맨 밑에 총계가 또 있다. 그 구조를 `boq` 에 끼워 넣으면
//! 돌아가던 물량표가 조건문 범벅이 된다.
//!
//! ── 단가 없는 칸은 `-` 다 ──
//!
//! 한: "단가는 일단 - 처리하고. 수량 이랑 규격 제대로 총합 잘 보이게"
//!
//! `unitPrice == 0` 은 **공짜가 아니라 아직 안 적은 것**이다. 0원이라고
//! 찍으면 받는 쪽이 둘을 못 가린다. 화면·종이와 같은 규칙으로 비운다
//! (`src/lib/purchaseOrder.ts` 의 `isPriced`).

use cadcraft_color::Color;
use cadcraft_doc::{Drawing, Entity, EntityKind, Table, TableCell, TableStyle};
use cadcraft_geom::{Vec2, Vec3};

use crate::boq::{
    Align, EDGE_PAD, FONT, HEADER_FILL, HEADER_H, INK, MUTED, PAD, ROW_H, RULE, TITLE_FILL,
    TITLE_FONT, TITLE_H, TOTAL_FILL, cell, fit_text, merged, style,
};
use crate::jsnum::to_fixed;

/// 발주서 품목 한 줄. 화면의 `PoRow` 와 같은 모양이다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PoRow {
    /// 구분 — 이 품목이 달린 물량표 이름 (석고보드 등).
    pub group: String,
    pub name: String,
    pub spec: String,
    pub qty: f64,
    pub unit: String,
    /// 단가. **0 은 "아직 안 적음"** 이다.
    pub unit_price: f64,
}

/// 발주서 하나. 좌표는 **캔버스** 것이다 — y 뒤집기는 이 안에서 한다.
#[derive(Clone, Debug, PartialEq)]
pub struct PoTable {
    /// 이 표를 만든 Bimove 도형의 id (`crate::identity`).
    pub id: String,
    /// 캔버스에서의 **좌상단** (도형의 `x,y`).
    pub at: Vec2,
    pub title: String,
    pub rows: Vec<PoRow>,
    pub layer: String,
}

struct Column {
    label: &'static str,
    width: f64,
    align: Align,
}

/// 폭 합이 612 — `boq::COLUMNS` 와 같다. 두 표를 나란히 세우면 변이 맞는다.
const COLUMNS: [Column; 7] = [
    Column { label: "구분", width: 88.0, align: Align::Left },
    Column { label: "품목", width: 104.0, align: Align::Left },
    Column { label: "규격", width: 120.0, align: Align::Left },
    Column { label: "수량", width: 66.0, align: Align::Right },
    Column { label: "단위", width: 48.0, align: Align::Center },
    Column { label: "단가", width: 86.0, align: Align::Right },
    Column { label: "금액", width: 100.0, align: Align::Right },
];

/// 표 스타일 이름. 물량표와 **다른 이름**이어야 한다 — 같은 이름이면 뒤에
/// 넣는 쪽이 앞의 스타일을 갈아끼운다 (`register` 가 그렇게 돼 있다).
pub const STYLE: &str = "BIMOVE_PO";

const TITLE_STYLE: &str = "BIMOVE_PO_TITLE";
const EMPTY_STYLE: &str = "BIMOVE_PO_EMPTY";
/// 소계줄 — 표 하나가 끝날 때마다 한 줄.
const SUB_L: &str = "BIMOVE_PO_SUB_L";
const SUB_M: &str = "BIMOVE_PO_SUB_M";
const SUB_R: &str = "BIMOVE_PO_SUB_R";
/// 총계줄 — 맨 밑 한 줄.
const TOTAL_L: &str = "BIMOVE_PO_TOTAL_L";
const TOTAL_M: &str = "BIMOVE_PO_TOTAL_M";
const TOTAL_R: &str = "BIMOVE_PO_TOTAL_R";

fn head_style(align: Align) -> String {
    format!("{STYLE}_HEAD_{}", align.suffix())
}
fn data_style(align: Align) -> String {
    format!("{STYLE}_DATA_{}", align.suffix())
}

fn table_width() -> f64 {
    COLUMNS.iter().map(|c| c.width).sum()
}

/// 단가를 정했나. **0 은 공짜가 아니라 아직 안 적은 것**이다.
fn priced(r: &PoRow) -> bool {
    r.unit_price > 0.0
}

/// 화면의 `fmtQty` 와 같은 글자.
///
/// 정수는 **소수점 손질에 아예 안 들어간다.** 꼬리 0 을 떼는 규칙을 정수에도
/// 먹이면 `100` 이 `1` 이 된다 (화면 쪽에서 한 번 밟았다).
pub(crate) fn fmt_qty(n: f64) -> String {
    if !n.is_finite() || n <= 0.0 {
        return "0".into();
    }
    if n.fract() == 0.0 && n.abs() < 9e15 {
        return format!("{}", n as i64);
    }
    let s = to_fixed(n, 2);
    let s = s.trim_end_matches('0');
    s.trim_end_matches('.').to_string()
}

/// 세 자리마다 쉼표. 화면의 `toLocaleString('ko-KR')` 자리다.
pub(crate) fn comma(n: f64) -> String {
    if !n.is_finite() {
        return "0".into();
    }
    let neg = n < 0.0;
    // 금액은 원 단위다 — 화면도 반올림해서 찍는다.
    let whole = n.abs().round();
    if whole >= 9e15 {
        return to_fixed(n, 0);
    }
    let digits = format!("{}", whole as u64);
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if neg { format!("-{out}") } else { out }
}

/// 단위별 물량 합 — 장과 m² 는 더할 수 있는 것이 아니다.
///
/// 나온 차례를 지킨다. 화면의 `quantityTotals` 와 같다.
fn quantity_totals(rows: &[&PoRow]) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = Vec::new();
    for r in rows {
        if r.qty <= 0.0 || !r.qty.is_finite() {
            continue;
        }
        let unit = if r.unit.is_empty() { "EA" } else { r.unit.as_str() };
        match out.iter_mut().find(|(u, _)| u == unit) {
            Some((_, q)) => *q += r.qty,
            None => out.push((unit.to_string(), r.qty)),
        }
    }
    // 화면은 `+o.qty.toFixed(2)` 로 깎는다 — 더하다 생긴 0.30000000000000004 를 지운다.
    for (_, q) in out.iter_mut() {
        *q = to_fixed(*q, 2).parse().unwrap_or(*q);
    }
    out
}

/// "15장 · 3.5m²". 셀 게 없으면 빈 문자열 — 부르는 쪽이 `-` 를 고른다.
fn summarize_qty(rows: &[&PoRow]) -> String {
    quantity_totals(rows)
        .iter()
        .map(|(u, q)| format!("{}{u}", fmt_qty(*q)))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn amount(r: &PoRow) -> f64 {
    r.qty * r.unit_price
}

fn sum_amount(rows: &[&PoRow]) -> f64 {
    rows.iter().map(|r| amount(r)).sum()
}

/// 한 칸에 찍을 글자.
fn cell_text(r: &PoRow, col: usize) -> String {
    match col {
        0 => r.group.clone(),
        1 => r.name.clone(),
        2 => r.spec.clone(),
        3 => fmt_qty(r.qty),
        4 => r.unit.clone(),
        5 => if priced(r) { comma(r.unit_price) } else { "-".into() },
        _ => if priced(r) { comma(amount(r)) } else { "-".into() },
    }
}

fn register(drawing: &mut Drawing) {
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
    // 소계·총계는 칸을 쓰지 않고 줄을 가로질러 적는다 — 세로 괘선을 끈다
    // (`boq` 의 합계줄과 같은 까닭이다. 안 끄면 글자를 괘선이 관통한다).
    for (l, m, r, fill) in [
        (SUB_L, SUB_M, SUB_R, HEADER_FILL),
        (TOTAL_L, TOTAL_M, TOTAL_R, TOTAL_FILL),
    ] {
        st.set_cell_style(style(l, FONT, Align::Left, Some(fill), INK, EDGE_PAD, (true, false)));
        st.set_cell_style(style(m, FONT, Align::Right, Some(fill), INK, PAD, (false, false)));
        st.set_cell_style(style(r, FONT, Align::Right, Some(fill), INK, EDGE_PAD, (false, true)));
    }

    match drawing.table_styles.iter_mut().find(|s| s.name == st.name) {
        Some(old) => *old = st,
        None => drawing.table_styles.push(st),
    }
}

/// 소계·총계 한 줄 — 라벨 / 물량 / 금액.
///
/// **줄은 칸 수만큼 길어야 한다.** `cells[row][i]` 의 `i` 는 열 번호라, 병합에
/// 먹힌 칸도 자리를 차지한다. 짧게 넣으면 뒤 칸이 앞으로 밀려 병합 범위에
/// 삼켜지고 **아예 안 그려진다** (물량표에서 합계 금액이 그렇게 사라졌다).
fn sum_row(label: &str, qty: &str, money: &str, styles: (&str, &str, &str)) -> Vec<TableCell> {
    let mut row = vec![TableCell::default(); COLUMNS.len()];
    // 구분·품목·규격 — 라벨
    row[0] = merged(label, styles.0, 3);
    // 수량·단위 — 물량 요약. 단가 칸은 비우고 금액 칸에 돈을 적는다.
    row[3] = merged(qty, styles.1, 3);
    row[6] = cell(money, styles.2);
    row
}

/// 본문 — 표(구분)마다 품목 줄을 깔고 끝에 소계를 붙인다.
///
/// 구분 칸은 **그 표의 첫 줄에만** 적는다. 줄마다 같은 이름을 되풀이하면
/// 눈이 어디서 표가 갈리는지 못 찾는다.
fn body_rows(t: &PoTable) -> Vec<Vec<TableCell>> {
    if t.rows.is_empty() {
        return vec![vec![merged("발주할 품목이 없습니다", EMPTY_STYLE, COLUMNS.len() as u32)]];
    }

    let mut out: Vec<Vec<TableCell>> = Vec::new();
    let mut i = 0;
    while i < t.rows.len() {
        let group = t.rows[i].group.clone();
        let mut j = i;
        while j < t.rows.len() && t.rows[j].group == group {
            j += 1;
        }
        let chunk: Vec<&PoRow> = t.rows[i..j].iter().collect();

        for (k, r) in chunk.iter().enumerate() {
            out.push(
                COLUMNS
                    .iter()
                    .enumerate()
                    .map(|(c, col)| {
                        let text = if c == 0 && k > 0 { String::new() } else { cell_text(r, c) };
                        cell(fit_text(&text, col.width), &data_style(col.align))
                    })
                    .collect(),
            );
        }

        let qty = summarize_qty(&chunk);
        let money = if chunk.iter().any(|r| priced(r)) { comma(sum_amount(&chunk)) } else { "-".into() };
        out.push(sum_row(
            &fit_text(&format!("{group} 소계"), COLUMNS[0].width + COLUMNS[1].width + COLUMNS[2].width),
            if qty.is_empty() { "-" } else { &qty },
            &money,
            (SUB_L, SUB_M, SUB_R),
        ));

        i = j;
    }
    out
}

fn total_row(t: &PoTable) -> Vec<TableCell> {
    let all: Vec<&PoRow> = t.rows.iter().collect();
    let qty = summarize_qty(&all);
    let money = if all.iter().any(|r| priced(r)) {
        format!("{}원", comma(sum_amount(&all)))
    } else {
        "-".into()
    };
    sum_row(
        "총 합계",
        if qty.is_empty() { "-" } else { &qty },
        &money,
        (TOTAL_L, TOTAL_M, TOTAL_R),
    )
}

/// 쓸 수 있는 표인가. 자리가 NaN 이면 어디에 그릴지 알 수 없다.
pub fn usable(t: &PoTable) -> bool {
    t.at.is_finite()
}

fn entity_for(t: &PoTable) -> Table {
    let mut cells: Vec<Vec<TableCell>> =
        vec![vec![merged(fit_text(&t.title, table_width()), TITLE_STYLE, COLUMNS.len() as u32)]];
    cells.push(COLUMNS.iter().map(|c| cell(c.label, &head_style(c.align))).collect());
    let body = body_rows(t);
    let mut heights = vec![TITLE_H, HEADER_H];
    heights.extend(body.iter().map(|_| ROW_H));
    heights.push(ROW_H);
    cells.extend(body);
    cells.push(total_row(t));

    Table {
        // 표는 넣은 점에서 **아래로** 자란다. 캔버스의 좌상단이 그대로 여기다.
        insert: Vec3::new(t.at.x, -t.at.y, 0.0),
        col_widths: COLUMNS.iter().map(|c| c.width).collect(),
        row_heights: heights,
        cells,
        style: STYLE.into(),
        text_height: FONT,
        title: true,
        header: true,
        row_styles: Vec::new(),
        breaks: None,
        data_link: None,
    }
}

/// 발주서를 도면에 넣는다.
///
/// @returns 넣은 개수. 못 쓰는 것은 세지 않는다.
pub fn add_tables(drawing: &mut Drawing, tables: &[PoTable]) -> usize {
    if !tables.iter().any(usable) {
        return 0;
    }
    register(drawing);

    let mut n = 0;
    for t in tables.iter().filter(|t| usable(t)) {
        let h = drawing.new_handle();
        let mut e = Entity::new(h, EntityKind::Table(entity_for(t)));
        e.common.layer = t.layer.clone();
        // 괘선색. 셀 스타일의 테두리는 `ByBlock` 이라 이걸 따라간다.
        e.common.color = Color::True(RULE);
        let from = drawing.model.len();
        drawing.model.push(e);
        crate::identity::stamp_one(drawing, from, crate::identity::PO, &t.id);
        n += 1;
    }
    n
}

/// 줄 수에서 나오는 표 크기. 화면(`poTableSize`)과 **같은 식**이어야 한다 —
/// 도형의 `w`/`h` 는 화면이 정하고 엔진은 상수로 다시 세우므로, 어긋나면
/// 고르기 테두리와 그려진 표가 다른 크기가 된다.
pub fn table_size(rows: &[PoRow]) -> (f64, f64) {
    let t = PoTable {
        id: String::new(),
        at: Vec2::new(0.0, 0.0),
        title: String::new(),
        rows: rows.to_vec(),
        layer: String::new(),
    };
    let body = body_rows(&t).len() as f64;
    (table_width(), TITLE_H + HEADER_H + body * ROW_H + ROW_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(group: &str, name: &str, spec: &str, qty: f64, unit: &str, price: f64) -> PoRow {
        PoRow {
            group: group.into(),
            name: name.into(),
            spec: spec.into(),
            qty,
            unit: unit.into(),
            unit_price: price,
        }
    }

    fn table(rows: Vec<PoRow>) -> PoTable {
        PoTable {
            id: String::new(),
            at: Vec2::new(100.0, 50.0),
            title: "발주서".into(),
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

    /// 표의 모든 글자를 한 줄로 — 무엇이 찍혔나 통째로 보려고.
    fn texts(t: &Table) -> Vec<String> {
        t.cells.iter().flatten().map(|c| c.text.clone()).collect()
    }

    #[test]
    fn 표를_하나_넣는다() {
        let mut d = Drawing::new_metric();
        assert_eq!(add_tables(&mut d, &[table(vec![row("석고보드", "석고보드", "900×1800", 12.0, "장", 0.0)])]), 1);
        let t = only(&d);
        assert_eq!(t.style, STYLE);
        assert!(t.title && t.header);
        assert_eq!(t.col_widths.len(), 7);
    }

    /// 물량표와 **폭이 같아야** 도면틀 옆에 나란히 세울 수 있다.
    #[test]
    fn 물량표와_폭이_같다() {
        assert_eq!(table_width(), 612.0);
    }

    /// 물량표와 **스타일 이름이 달라야** 한다. 같으면 뒤에 넣는 쪽이 앞의
    /// 스타일을 갈아끼워서, 두 표 중 하나가 남의 칸 모양으로 그려진다.
    #[test]
    fn 물량표와_스타일_이름이_다르다() {
        assert_ne!(STYLE, crate::boq::STYLE);
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "나", "다", 1.0, "장", 0.0)])]);
        crate::boq::add_tables(
            &mut d,
            &[crate::boq::BoqTable {
                id: String::new(),
                at: Vec2::new(0.0, 0.0),
                title: "물량".into(),
                rows: Vec::new(),
                total_area_m2: 0.0,
                layer: "0".into(),
            }],
        );
        // `Drawing::new_metric` 이 기본 스타일을 하나 깔아 두므로 수가 아니라
        // **이름 둘이 다 있는지**를 본다.
        let names: Vec<&str> = d.table_styles.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&STYLE), "{names:?}");
        assert!(names.contains(&crate::boq::STYLE), "스타일이 덮였다: {names:?}");
    }

    #[test]
    fn 줄_높이가_화면과_같다() {
        // 제목 + 머리 + (품목 2 + 소계 1) + 총계
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![
            row("석고", "석고보드", "900×1800", 2.0, "장", 0.0),
            row("석고", "피스", "", 1.0, "box", 0.0),
        ])]);
        let t = only(&d);
        assert_eq!(t.row_heights, vec![28.0, 24.0, 22.0, 22.0, 22.0, 22.0]);
    }

    /// 표마다 소계가 하나씩 붙고, 맨 밑에 총계가 또 하나.
    #[test]
    fn 표마다_소계가_붙는다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![
            row("석고보드", "석고보드", "", 1.0, "장", 0.0),
            row("타일", "벽 타일", "", 1.0, "box", 0.0),
        ])]);
        let all = texts(&only(&d));
        assert!(all.iter().any(|s| s == "석고보드 소계"), "{all:?}");
        assert!(all.iter().any(|s| s == "타일 소계"), "{all:?}");
        assert!(all.iter().any(|s| s == "총 합계"), "{all:?}");
    }

    /// 구분 칸은 그 표의 **첫 줄에만**. 줄마다 되풀이하면 어디서 갈리는지
    /// 눈이 못 찾는다.
    #[test]
    fn 구분은_첫_줄에만_적는다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![
            row("석고보드", "9.5T", "", 1.0, "장", 0.0),
            row("석고보드", "12.5T", "", 1.0, "장", 0.0),
        ])]);
        let t = only(&d);
        // 0=제목, 1=머리, 2·3=품목, 4=소계, 5=총계
        assert_eq!(t.cells[2][0].text, "석고보드");
        assert_eq!(t.cells[3][0].text, "", "둘째 줄에도 구분을 적었다");
    }

    /// **0원이 아니라 `-`.** 단가를 안 적은 것과 공짜는 다르다.
    #[test]
    fn 단가를_안_적었으면_금액이_빈다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("석고", "석고보드", "", 12.0, "장", 0.0)])]);
        let all = texts(&only(&d));
        assert!(!all.iter().any(|s| s.contains('0') && s.contains('원')), "{all:?}");
        assert!(all.iter().filter(|s| *s == "-").count() >= 3, "단가·금액·소계가 안 비었다: {all:?}");
        // 물량은 그대로 선다
        assert!(all.iter().any(|s| s == "12장"), "{all:?}");
    }

    #[test]
    fn 단가를_적으면_금액이_돌아온다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("석고", "석고보드", "", 10.0, "장", 3000.0)])]);
        let all = texts(&only(&d));
        assert!(all.iter().any(|s| s == "30,000"), "{all:?}");
        assert!(all.iter().any(|s| s == "30,000원"), "총계에 원이 안 붙었다: {all:?}");
    }

    /// 장과 m² 는 더할 수 있는 것이 아니다.
    #[test]
    fn 단위가_섞이면_따로_센다() {
        let rows = vec![
            row("가", "ㄱ", "", 12.0, "장", 0.0),
            row("가", "ㄴ", "", 3.5, "m²", 0.0),
            row("가", "ㄷ", "", 1.0, "장", 0.0),
        ];
        let refs: Vec<&PoRow> = rows.iter().collect();
        assert_eq!(summarize_qty(&refs), "13장 · 3.5m²");
    }

    #[test]
    fn 단위를_안_적었으면_ea() {
        let rows = vec![row("가", "ㄱ", "", 2.0, "", 0.0)];
        let refs: Vec<&PoRow> = rows.iter().collect();
        assert_eq!(summarize_qty(&refs), "2EA");
    }

    /// 화면의 `fmtQty` 와 **같은 글자**여야 한다. 꼬리 0 을 떼는 규칙이
    /// 정수까지 먹으면 `100` 이 `1` 이 된다.
    #[test]
    fn 수량_글자가_화면과_같다() {
        assert_eq!(fmt_qty(12.0), "12");
        assert_eq!(fmt_qty(10.0), "10");
        assert_eq!(fmt_qty(100.0), "100");
        assert_eq!(fmt_qty(1000.0), "1000");
        assert_eq!(fmt_qty(3.5), "3.5");
        assert_eq!(fmt_qty(1.25), "1.25");
        assert_eq!(fmt_qty(0.125), "0.13");
        assert_eq!(fmt_qty(0.0), "0");
        assert_eq!(fmt_qty(-5.0), "0");
        assert_eq!(fmt_qty(f64::NAN), "0");
    }

    #[test]
    fn 금액에_세_자리마다_쉼표() {
        assert_eq!(comma(0.0), "0");
        assert_eq!(comma(999.0), "999");
        assert_eq!(comma(1000.0), "1,000");
        assert_eq!(comma(45000.0), "45,000");
        assert_eq!(comma(1234567.0), "1,234,567");
        assert_eq!(comma(-1500.0), "-1,500");
    }

    /// 빈 발주서도 줄 하나는 그린다 — 선 하나로 쪼그라들면 도면에서 못 찾는다.
    #[test]
    fn 빈_발주서도_한_줄은_그린다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(Vec::new())]);
        let t = only(&d);
        assert_eq!(t.row_heights.len(), 4, "제목·머리·안내·총계");
        assert!(texts(&t).iter().any(|s| s.contains("품목이 없습니다")));
    }

    /// 병합에 먹힌 칸도 **자리를 차지한다.** 줄을 짧게 넣으면 뒤 칸이 앞으로
    /// 밀려 삼켜지고 금액이 아예 안 그려진다.
    #[test]
    fn 합계줄이_칸_수만큼_길다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "ㄱ", "", 1.0, "장", 100.0)])]);
        let t = only(&d);
        for (i, r) in t.cells.iter().enumerate() {
            assert_eq!(r.len().min(COLUMNS.len()), if i == 0 { 1 } else { COLUMNS.len() },
                "{i}번 줄의 칸 수: {}", r.len());
        }
    }

    #[test]
    fn 자리가_nan_이면_안_그린다() {
        let mut d = Drawing::new_metric();
        let mut t = table(vec![row("가", "ㄱ", "", 1.0, "장", 0.0)]);
        t.at = Vec2::new(f64::NAN, 0.0);
        assert_eq!(add_tables(&mut d, &[t]), 0);
        assert!(d.model.is_empty());
    }

    /// 자리는 캔버스 좌상단 그대로, y 만 뒤집는다.
    #[test]
    fn 캔버스_자리에_놓는다() {
        let mut d = Drawing::new_metric();
        add_tables(&mut d, &[table(vec![row("가", "ㄱ", "", 1.0, "장", 0.0)])]);
        let t = only(&d);
        assert_eq!(t.insert.x, 100.0);
        assert_eq!(t.insert.y, -50.0);
    }

    /// 크기 식이 본문 줄 수를 따라간다 (소계줄도 센다).
    #[test]
    fn 표_크기가_줄_수를_따라간다() {
        let (w, h) = table_size(&[
            row("가", "ㄱ", "", 1.0, "장", 0.0),
            row("나", "ㄴ", "", 1.0, "장", 0.0),
        ]);
        assert_eq!(w, 612.0);
        // 제목 28 + 머리 24 + (품목 2 + 소계 2) × 22 + 총계 22
        assert_eq!(h, 28.0 + 24.0 + 4.0 * 22.0 + 22.0);
    }

    #[test]
    fn 빈_표의_크기도_한_줄은_잡는다() {
        let (_, h) = table_size(&[]);
        assert_eq!(h, 28.0 + 24.0 + 22.0 + 22.0);
    }
}
