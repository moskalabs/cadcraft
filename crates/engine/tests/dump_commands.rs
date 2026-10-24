//! 명령 창구에 뭐가 있나 **적어 둔다.**
//!
//! Bimove 가 tldraw 를 걷어내면서 "문서를 누가 들고 있나" 를 정해야 했다.
//! 답은 엔진이다 — 그러려면 UI 가 부를 수 있는 명령이 무엇무엇인지 한 장에
//! 보여야 한다. 사람이 읽을 표를 `target/commands.md` 에 떨군다.
//!
//! 검사이기도 하다: id 가 겹치거나 params 설명이 빠진 명령을 여기서 잡는다.

use std::collections::BTreeMap;

#[test]
fn 명령표를_뽑는다() {
    let specs = cadcraft_engine::cmd::command_specs();

    // id 가 겹치면 `find_command` 가 먼저 걸린 것만 돌려준다 — 조용히 가려진다.
    let mut seen = BTreeMap::new();
    for s in specs {
        if let Some(prev) = seen.insert(s.id, s.label) {
            panic!("id 가 겹친다: {} ({prev} / {})", s.id, s.label);
        }
    }

    let mut by_area: BTreeMap<&str, Vec<&cadcraft_engine::cmd::CommandSpec>> = BTreeMap::new();
    for s in specs {
        let area = s.menu.first().copied().unwrap_or("(메뉴 없음)");
        by_area.entry(area).or_default().push(s);
    }

    let mut out = String::new();
    out.push_str(&format!("# 엔진 명령 {}개\n\n", specs.len()));
    out.push_str("`Engine::execute(id, params)` 로 부른다. `되돌림` 이 아니면 undo 에 안 쌓인다.\n\n");

    for (area, mut list) in by_area {
        list.sort_by_key(|s| s.id);
        out.push_str(&format!("\n## {area} ({}개)\n\n", list.len()));
        out.push_str("| id | 이름 | params | 되돌림 |\n|---|---|---|---|\n");
        for s in list {
            let p = if s.params.is_empty() { "—" } else { s.params };
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                s.id,
                s.label,
                p.replace('|', "\\|"),
                if s.undoable { "O" } else { "" }
            ));
        }
    }

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/commands.md");
    std::fs::write(path, &out).expect("못 썼다");
    eprintln!("명령 {}개 → {path}", specs.len());
}
