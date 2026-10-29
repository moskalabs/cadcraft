//! Defpoints always goes out as non-plotting.
//!
//! AutoCAD pins Defpoints as a never-plotting layer. A LAYER record that claims
//! otherwise is refused — "invalid AcDbLayerTableRecord plot flag for layer
//! DEFPOINTS" — and the whole drawing is dropped, not just that layer.

use cadcraft_doc::Drawing;
use cadcraft_io::{read_dxf, write_dxf};

/// The group pairs of the LAYER table record named `name`, codes as strings.
fn layer_record(dxf: &str, name: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    let pairs: Vec<(String, String)> =
        lines.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].to_string(), c[1].to_string())).collect();
    let mut cur: Vec<(String, String)> = Vec::new();
    let mut found: Option<Vec<(String, String)>> = None;
    for (code, val) in pairs {
        if code == "0" {
            if cur.iter().any(|(c, v)| c == "2" && v.eq_ignore_ascii_case(name)) {
                found = Some(std::mem::take(&mut cur));
            }
            cur.clear();
            if val == "LAYER" {
                cur.push((code, val));
            }
            continue;
        }
        if !cur.is_empty() {
            cur.push((code, val));
        }
    }
    found.unwrap_or_default()
}

#[test]
fn defpoints_is_written_non_plotting_even_when_the_doc_says_otherwise() {
    let mut d = Drawing::new_metric();
    let l = d.layers.iter_mut().find(|l| l.name.eq_ignore_ascii_case("Defpoints")).expect("Defpoints");
    // A writer that defaults a fresh layer to plottable produces exactly this.
    l.plot = true;

    let dxf = write_dxf(&d);
    let rec = layer_record(&dxf, "Defpoints");
    assert!(!rec.is_empty(), "no Defpoints LAYER record");
    assert!(
        rec.iter().any(|(c, v)| c == "290" && v == "0"),
        "Defpoints must carry 290 0; got {:?}",
        rec.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>()
    );

    let back = read_dxf(dxf.as_bytes()).expect("reopen");
    let dp = back.layers.iter().find(|l| l.name.eq_ignore_ascii_case("Defpoints")).expect("Defpoints back");
    assert!(!dp.plot, "reopens non-plotting");
}

#[test]
fn an_ordinary_plotting_layer_keeps_its_flag() {
    let mut d = Drawing::new_metric();
    d.layers.push(cadcraft_doc::Layer::new("A-WALL"));
    let dxf = write_dxf(&d);
    let rec = layer_record(&dxf, "A-WALL");
    assert!(!rec.is_empty(), "no A-WALL LAYER record");
    assert!(!rec.iter().any(|(c, v)| c == "290" && v == "0"), "A-WALL plots; got {rec:?}");
}
