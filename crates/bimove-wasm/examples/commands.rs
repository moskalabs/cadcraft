//! 엔진이 내주는 명령 목록을 뽑아 본다. 어떤 걸 바인딩할지 고르는 용도.
//!
//! 쓰기: cargo run -p bimove-wasm --example commands -- [검색어]

fn main() {
    let needle = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let specs = cadcraft_engine::cmd::command_specs();
    println!("명령 {}개\n", specs.len());

    let mut shown = 0;
    for s in specs {
        if !needle.is_empty() && !s.id.contains(&needle) && !s.label.to_lowercase().contains(&needle) {
            continue;
        }
        println!(
            "{:<16} {:<34} params={:<28} undo={} 대화식={}",
            s.id,
            s.label,
            if s.params.is_empty() { "-" } else { s.params },
            if s.undoable { "O" } else { "." },
            if s.interactive.is_some() { "O" } else { "." },
        );
        shown += 1;
        if shown >= 60 {
            println!("… (더 있음)");
            break;
        }
    }
    if shown == 0 {
        println!("'{needle}' 에 맞는 명령 없음");
    }
}
