//! Extended data: a JSON value attached to an object under a registered application.
//!
//! A CAD object is a line, a polyline, a circle. An application built on top of one usually needs
//! to say more: which wall this polyline is, what it is finished with, what it costs. DXF's place
//! for that is xdata — group `1001 <APPID>` and the groups after it, carried by the object through
//! copies (COPY, MIRROR, ARRAY, paste) and written back out to the file.
//!
//! The commands here store a whole JSON value instead of exposing group codes. `1000` strings hold
//! at most 255 bytes, so the text is cut into chunks on character boundaries and joined again on
//! the way back. An application that wants raw group codes still has them in `Common::xdata`.
//!
//! The APPID table entry is not written here: the DXF writer registers every application that has
//! xdata in the drawing.

use cadcraft_doc::RawTag;
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

/// Where Bimove (and anything else that does not name an application) puts its data.
pub const DEFAULT_APP: &str = "BIMOVE";

/// A `1000` group holds 255 bytes at most.
const CHUNK: usize = 255;

/// DXF group code for an xdata application name — the start of a run.
const APP_CODE: i32 = 1001;
/// DXF group code for an xdata string.
const STR_CODE: i32 = 1000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("xdata", "Extended Data", run_get)
            .params("{handles? (default the selection), app?: \"BIMOVE\"} → {count, objects: [{handle, data}]} (data is null where the object has none)")
            .noundo(),
        CommandSpec::new("xdata.set", "Set Extended Data", run_set)
            .params("{handles? (default the selection), app?: \"BIMOVE\", data: any JSON value | null to remove} → {count}"),
    ]
}

/// The application name a call means.
fn app_of(p: &Value) -> Result<String> {
    let name = p.get("app").and_then(Value::as_str).unwrap_or(DEFAULT_APP).trim();
    // An empty or over-long name cannot be written to the APPID table, and the data would be
    // dropped on save without anything having failed.
    if name.is_empty() || name.len() > 255 {
        return Err(EngineError::Other("`app` must be 1 to 255 characters".into()));
    }
    Ok(name.to_string())
}

/// The `1000` strings of `app`'s run, joined.
///
/// A run starts at its `1001` and ends at the next one — applications do not interleave.
fn read(tags: &[RawTag], app: &str) -> Option<String> {
    let start = tags.iter().position(|t| t.code == APP_CODE && t.value.eq_ignore_ascii_case(app))?;
    let rest = &tags[start + 1..];
    let end = rest.iter().position(|t| t.code == APP_CODE).unwrap_or(rest.len());
    let text: String = rest[..end].iter().filter(|t| t.code == STR_CODE).map(|t| t.value.as_str()).collect();
    Some(text)
}

/// `app`'s run replaced by `tags` (or removed, when empty). Other applications keep their place.
pub fn splice(old: &[RawTag], app: &str, tags: Vec<RawTag>) -> Vec<RawTag> {
    let mut out = Vec::with_capacity(old.len() + tags.len());
    let mut i = 0;
    while i < old.len() {
        if old[i].code == APP_CODE && old[i].value.eq_ignore_ascii_case(app) {
            // Skip the whole run, up to the next application.
            i += 1;
            while i < old.len() && old[i].code != APP_CODE {
                i += 1;
            }
            continue;
        }
        out.push(old[i].clone());
        i += 1;
    }
    out.extend(tags);
    out
}

/// `text` cut into `1000` groups on character boundaries.
///
/// Cutting by bytes would split a multi-byte character in two, and the halves do not join back
/// into the character they came from — Korean and Japanese text would come back as garbage.
fn chunks(text: &str) -> Vec<RawTag> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut cut = rest.len().min(CHUNK);
        while cut > 0 && !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        // A single character wider than the chunk cannot happen (4 bytes at most), but a zero cut
        // would loop for ever if it ever did.
        if cut == 0 {
            cut = rest.chars().next().map_or(rest.len(), char::len_utf8);
        }
        out.push(RawTag { code: STR_CODE, value: rest[..cut].to_string() });
        rest = &rest[cut..];
    }
    out
}

/// `data` as `app`'s whole run of groups, ready to [`splice`] in.
///
/// Public because not every producer goes through a command. Code that assembles a [`Drawing`]
/// in one pass — a converter from another program's format — has no handles to name yet and so
/// cannot call `xdata.set`. It still has to write the **same bytes**, or what it stores will not
/// be what `xdata` reads back.
///
/// [`Drawing`]: cadcraft_doc::Drawing
pub fn encode(app: &str, data: &Value) -> Vec<RawTag> {
    if data.is_null() {
        return Vec::new();
    }
    let mut v = vec![RawTag { code: APP_CODE, value: app.to_string() }];
    // `to_string` on a `Value` cannot fail — the value is already parsed JSON.
    v.extend(chunks(&data.to_string()));
    v
}

/// `app`'s run read back as JSON, or `None` where the object has none.
///
/// Text that is not JSON is another application's, or ours from before a format change. It comes
/// back as a string rather than being dropped.
pub fn decode(tags: &[RawTag], app: &str) -> Option<Value> {
    read(tags, app).map(|t| serde_json::from_str::<Value>(&t).unwrap_or(Value::String(t)))
}

fn run_get(s: &mut Session, p: &Value) -> Result<Value> {
    let app = app_of(p)?;
    let hs = targets(s, p)?;
    let d = s.doc()?;
    let objects: Vec<Value> = hs
        .iter()
        .filter_map(|h| d.entity(*h).map(|e| (h, e)))
        .map(|(h, e)| {
            let data = decode(&e.common.xdata, &app).unwrap_or(Value::Null);
            json!({ "handle": h.hex(), "data": data })
        })
        .collect();
    Ok(json!({ "count": objects.len(), "objects": objects }))
}

fn run_set(s: &mut Session, p: &Value) -> Result<Value> {
    let app = app_of(p)?;
    let hs = targets(s, p)?;
    if hs.is_empty() {
        return Err(EngineError::Other("xdata.set: nothing selected".into()));
    }
    // A missing `data` is a caller's mistake; `data: null` means remove. Telling them apart keeps
    // a typo in the key from silently wiping what is there.
    let Some(data) = p.get("data") else {
        return Err(EngineError::Other("xdata.set: `data` is required (use null to remove)".into()));
    };

    let tags = encode(&app, data);

    let d = s.doc_mut()?;
    // `modify_entity` already refuses a handle that is not there, so its result is the count —
    // a handle the caller made up is skipped, not an error for the whole call.
    let n = hs
        .iter()
        .filter(|h| d.modify_entity(**h, |e| e.common.xdata = splice(&e.common.xdata, &app, tags.clone())).is_ok())
        .count();
    Ok(json!({ "count": n }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(code: i32, v: &str) -> RawTag {
        RawTag { code, value: v.to_string() }
    }

    #[test]
    fn long_text_is_cut_on_character_boundaries() {
        // Cutting by bytes would split a multi-byte character and the halves do not join back.
        let text = "벽".repeat(300);
        let cs = chunks(&text);
        assert!(cs.len() > 1, "300 characters should not fit one group");
        for c in &cs {
            assert!(c.value.len() <= CHUNK, "a group holds 255 bytes at most");
        }
        let joined: String = cs.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(joined, text, "the chunks do not join back");
    }

    #[test]
    fn other_applications_keep_their_data() {
        // Replacing ours must not disturb anyone else's — theirs is their file's, not ours to drop.
        let old = vec![tag(1001, "ACAD"), tag(1000, "theirs"), tag(1001, "BIMOVE"), tag(1000, "ours")];
        let out = splice(&old, "BIMOVE", vec![tag(1001, "BIMOVE"), tag(1000, "new")]);
        assert_eq!(read(&out, "ACAD").as_deref(), Some("theirs"));
        assert_eq!(read(&out, "BIMOVE").as_deref(), Some("new"));
    }

    #[test]
    fn removing_leaves_the_rest() {
        let old = vec![tag(1001, "BIMOVE"), tag(1000, "ours"), tag(1001, "ACAD"), tag(1000, "theirs")];
        let out = splice(&old, "BIMOVE", Vec::new());
        assert_eq!(read(&out, "BIMOVE"), None);
        assert_eq!(read(&out, "ACAD").as_deref(), Some("theirs"));
    }

    #[test]
    fn a_run_ends_at_the_next_application() {
        // Reading past the next 1001 would hand back another application's text as ours.
        let tags = vec![tag(1001, "BIMOVE"), tag(1000, "a"), tag(1000, "b"), tag(1001, "ACAD"), tag(1000, "c")];
        assert_eq!(read(&tags, "BIMOVE").as_deref(), Some("ab"));
    }
}
