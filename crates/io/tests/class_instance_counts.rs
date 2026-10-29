//! CLASS records: group 91 (`NumberOfInstances`) is **R2004 and later only**.
//!
//! In the R2000 CLASS record group 280 must follow 90 directly. Writing 91 into an AC1015 file
//! makes AutoCAD stop at the first class and throw the whole drawing away:
//!
//! ```text
//! Group 280 (zombie flag) expected at line 194.
//! Invalid or incomplete DXF input -- drawing discarded.
//! ```
//!
//! Every R2000 DXF we wrote hit this. It survived because lenient readers (ezdxf, our own) skip
//! the unexpected group — the file looks fine right up until AutoCAD opens it. So this test
//! reads the **group order**, not whether some parser accepts it.

use cadcraft_doc::{Drawing, Entity, EntityKind, Line};
use cadcraft_geom::Vec3;
use cadcraft_io::Version;

fn drawing() -> Drawing {
    let mut d = Drawing::new_metric();
    let h = d.new_handle();
    d.model.push(Entity::new(
        h,
        EntityKind::Line(Line { a: Vec3::new(0.0, 0.0, 0.0), b: Vec3::new(100.0, 0.0, 0.0) }),
    ));
    d
}

/// Group codes of every CLASS record, in file order.
fn class_groups(text: &str) -> Vec<Vec<i64>> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i] == "0" && lines[i + 1] == "CLASS" {
            let mut codes = Vec::new();
            let mut j = i + 2;
            while j + 1 < lines.len() && lines[j] != "0" {
                if let Ok(c) = lines[j].parse::<i64>() {
                    codes.push(c);
                }
                j += 2;
            }
            out.push(codes);
            i = j;
            continue;
        }
        i += 2;
    }
    out
}

#[test]
fn r2000_class_records_have_no_group_91() {
    let text = cadcraft_io::write_dxf(&drawing());
    assert!(text.contains("AC1015"), "R2000 으로 안 썼다");

    let classes = class_groups(&text);
    assert!(!classes.is_empty(), "CLASS 레코드가 하나도 없다");
    for (n, codes) in classes.iter().enumerate() {
        assert!(!codes.contains(&91), "CLASS {n} 에 그룹 91 이 있다: {codes:?}");
        // 90 다음은 **곧바로** 280 이라야 한다 — AutoCAD 가 거기서 멈춘다.
        let at90 = codes.iter().position(|c| *c == 90).unwrap_or_else(|| panic!("CLASS {n} 에 90 이 없다"));
        assert_eq!(codes.get(at90 + 1), Some(&280), "CLASS {n}: 90 다음이 280 이 아니다 — {codes:?}");
    }
}

#[test]
fn r2004_class_records_keep_group_91() {
    // 91 을 통째로 없애면 안 된다. R2004 부터는 있는 게 맞다.
    let opts = cadcraft_io::SaveOptions { version: Some(Version::R2004), binary: false };
    let bytes = cadcraft_io::write_with(&drawing(), "x.dxf", &opts).expect("R2004 로 못 썼다");
    let text = String::from_utf8_lossy(&bytes);

    let classes = class_groups(&text);
    assert!(!classes.is_empty(), "CLASS 레코드가 하나도 없다");
    assert!(
        classes.iter().any(|c| c.contains(&91)),
        "R2004 인데 그룹 91 이 하나도 없다",
    );
}
