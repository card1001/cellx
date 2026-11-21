//! A1-reference rewriting for formulas CellX keeps outside the calculation engine
//! (bridge formulas). IronCalc updates its own formulas when rows move or cells are
//! copied; these functions do the same for the text of bridge formulas, with Excel's
//! rules: relative parts follow copies, everything follows inserted/deleted rows, and a
//! reference to a deleted cell becomes `#REF!`.

pub const LAST_ROW: i32 = 1_048_576;
pub const LAST_COL: i32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

/// One end of a reference: `$B$4`, `B4`, a whole column `B` or a whole row `4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Corner {
    col: Option<(i32, bool)>, // (column, absolute)
    row: Option<(i32, bool)>, // (row, absolute)
}

impl Corner {
    fn render(&self) -> String {
        let mut s = String::new();
        if let Some((c, abs)) = self.col {
            if abs {
                s.push('$');
            }
            s.push_str(&column_name(c));
        }
        if let Some((r, abs)) = self.row {
            if abs {
                s.push('$');
            }
            s.push_str(&r.to_string());
        }
        s
    }
}

pub fn column_name(mut c: i32) -> String {
    let mut s = Vec::new();
    while c > 0 {
        let m = (c - 1) % 26;
        s.push((b'A' + m as u8) as char);
        c = (c - 1) / 26;
    }
    s.iter().rev().collect()
}

fn parse_corner(s: &str) -> Option<Corner> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut col = None;
    let mut row = None;
    let col_abs = b.get(i) == Some(&b'$');
    if col_abs {
        i += 1;
    }
    let start = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i > start {
        if i - start > 3 {
            return None;
        }
        let mut c = 0;
        for ch in s[start..i].to_ascii_uppercase().bytes() {
            c = c * 26 + (ch - b'A' + 1) as i32;
        }
        if c > LAST_COL {
            return None;
        }
        col = Some((c, col_abs));
    } else if col_abs {
        // `$4` is a whole-row reference with an absolute row.
        i = start - 1;
    }
    let row_abs = b.get(i) == Some(&b'$');
    if row_abs {
        i += 1;
    }
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i > start {
        let r: i32 = s[start..i].parse().ok()?;
        if !(1..=LAST_ROW).contains(&r) {
            return None;
        }
        row = Some((r, row_abs));
    } else if row_abs {
        return None;
    }
    if i != b.len() || (col.is_none() && row.is_none()) {
        return None;
    }
    Some(Corner { col, row })
}

/// A reference found in formula text, with its byte span.
#[derive(Debug, Clone)]
struct Found {
    start: usize,
    end: usize,
    sheet: Option<String>,
    sheet_text: String, // the prefix as written, including `!`
    a: Corner,
    b: Option<Corner>,
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// Finds every A1 reference outside string literals and structured-reference brackets.
fn find_refs(formula: &str) -> Vec<Found> {
    let chars: Vec<(usize, char)> = formula.char_indices().collect();
    let n = chars.len();
    let at = |i: usize| -> usize { if i < n { chars[i].0 } else { formula.len() } };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let c = chars[i].1;
        if c == '"' {
            i += 1;
            while i < n {
                if chars[i].1 == '"' {
                    if i + 1 < n && chars[i + 1].1 == '"' {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == '[' {
            let mut depth = 0;
            while i < n {
                if chars[i].1 == '[' {
                    depth += 1;
                } else if chars[i].1 == ']' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        let prev_ok = i == 0 || !(is_name_char(chars[i - 1].1) || chars[i - 1].1 == ']');
        if !prev_ok || !(c.is_alphanumeric() || c == '$' || c == '\'' || c == '_') {
            i += 1;
            continue;
        }
        // Optional sheet prefix.
        let start = i;
        let mut j = i;
        let mut sheet = None;
        if c == '\'' {
            let mut k = i + 1;
            let mut name = String::new();
            let mut closed = false;
            while k < n {
                if chars[k].1 == '\'' {
                    if k + 1 < n && chars[k + 1].1 == '\'' {
                        name.push('\'');
                        k += 2;
                        continue;
                    }
                    closed = true;
                    break;
                }
                name.push(chars[k].1);
                k += 1;
            }
            if closed && k + 1 < n && chars[k + 1].1 == '!' {
                sheet = Some(name);
                j = k + 2;
            } else {
                i += 1;
                continue;
            }
        } else {
            let mut k = i;
            while k < n && (is_name_char(chars[k].1) || chars[k].1 == '$') {
                k += 1;
            }
            if k < n && chars[k].1 == '!' {
                sheet = Some(formula[at(i)..at(k)].to_string());
                j = k + 1;
            }
        }
        let sheet_text = formula[at(start)..at(j)].to_string();
        // First corner.
        let mut k = j;
        while k < n && (chars[k].1.is_ascii_alphanumeric() || chars[k].1 == '$') {
            k += 1;
        }
        let first = &formula[at(j)..at(k)];
        let Some(a) = parse_corner(first) else {
            // Not a reference: skip the whole word so we do not match inside it.
            let mut w = i + 1;
            while w < n && is_name_char(chars[w].1) {
                w += 1;
            }
            i = w;
            continue;
        };
        // A function name such as LOG10( is not a reference.
        if k < n && chars[k].1 == '(' {
            i = k;
            continue;
        }
        let mut end = k;
        let mut b = None;
        if k < n && chars[k].1 == ':' {
            let mut m = k + 1;
            while m < n && (chars[m].1.is_ascii_alphanumeric() || chars[m].1 == '$') {
                m += 1;
            }
            if let Some(corner) = parse_corner(&formula[at(k + 1)..at(m)]) {
                b = Some(corner);
                end = m;
            }
        }
        // A bare column (`A`) or row (`4`) only counts as part of a range like A:A or 1:4.
        let whole = a.row.is_none() || a.col.is_none();
        if whole && b.is_none() {
            i = k.max(i + 1);
            continue;
        }
        // Words like `orders` are not references, but `orders[...]`-style names were skipped above.
        if end < n && (is_name_char(chars[end].1) || chars[end].1 == '[') {
            i = end;
            continue;
        }
        out.push(Found { start: at(start), end: at(end), sheet, sheet_text, a, b });
        i = end;
    }
    out
}

fn render(f: &Found, a: Corner, b: Option<Corner>) -> String {
    let mut s = f.sheet_text.clone();
    s.push_str(&a.render());
    if let Some(b) = b {
        s.push(':');
        s.push_str(&b.render());
    }
    s
}

fn rewrite(formula: &str, mut f: impl FnMut(&Found) -> Option<String>) -> String {
    let mut out = String::with_capacity(formula.len());
    let mut last = 0;
    for r in find_refs(formula) {
        out.push_str(&formula[last..r.start]);
        match f(&r) {
            Some(text) => out.push_str(&text),
            None => out.push_str(&formula[r.start..r.end]),
        }
        last = r.end;
    }
    out.push_str(&formula[last..]);
    out
}

/// Moves relative references by (`drow`, `dcol`), as when a formula is copied or filled.
pub fn offset_refs(formula: &str, drow: i32, dcol: i32) -> String {
    rewrite(formula, |r| {
        let move_corner = |c: Corner| -> Option<Corner> {
            let col = match c.col {
                Some((v, false)) => Some((v + dcol, false)),
                other => other,
            };
            let row = match c.row {
                Some((v, false)) => Some((v + drow, false)),
                other => other,
            };
            let ok = col.is_none_or(|(v, _)| (1..=LAST_COL).contains(&v))
                && row.is_none_or(|(v, _)| (1..=LAST_ROW).contains(&v));
            ok.then_some(Corner { col, row })
        };
        let a = move_corner(r.a);
        let b = r.b.map(move_corner);
        match (a, b) {
            (Some(a), None) => Some(render(r, a, None)),
            (Some(a), Some(Some(b))) => Some(render(r, a, Some(b))),
            _ => Some("#REF!".to_string()),
        }
    })
}

/// Paste with "행/열 바꿈": a formula copied from `src` to `dst` (row, col). Fully
/// relative references swap their row and column offsets, as Excel does; others move
/// like a normal paste.
pub fn transpose_refs(formula: &str, src: (i32, i32), dst: (i32, i32)) -> String {
    let plain = offset_refs(formula, dst.0 - src.0, dst.1 - src.1);
    let mut any_swapped = false;
    let swapped = rewrite(formula, |r| {
        let turn = |c: Corner| -> Option<Corner> {
            match (c.col, c.row) {
                (Some((col, false)), Some((row, false))) => {
                    let (dr, dc) = (row - src.0, col - src.1);
                    let nc = (dst.1 + dr, false);
                    let nr = (dst.0 + dc, false);
                    ((1..=LAST_COL).contains(&nc.0) && (1..=LAST_ROW).contains(&nr.0))
                        .then_some(Corner { col: Some(nc), row: Some(nr) })
                }
                _ => None,
            }
        };
        let a = turn(r.a);
        let b = r.b.map(turn);
        match (a, b) {
            (Some(a), None) => {
                any_swapped = true;
                Some(render(r, a, None))
            }
            (Some(a), Some(Some(b))) => {
                any_swapped = true;
                Some(render(r, a, Some(b)))
            }
            _ => None,
        }
    });
    if !any_swapped {
        return plain;
    }
    // Mixed formulas: swap the fully relative references, move the rest.
    let mut out = String::new();
    let refs_plain = find_refs(&plain);
    let refs_sw = find_refs(&swapped);
    if refs_plain.len() != refs_sw.len() {
        return swapped;
    }
    let orig = find_refs(formula);
    let mut last = 0;
    for (i, r) in refs_sw.iter().enumerate() {
        out.push_str(&swapped[last..r.start]);
        let fully_relative = |c: &Corner| matches!((c.col, c.row), (Some((_, false)), Some((_, false))));
        let use_swapped = orig.get(i).is_some_and(|o| fully_relative(&o.a) && o.b.as_ref().is_none_or(fully_relative));
        if use_swapped {
            out.push_str(&swapped[r.start..r.end]);
        } else {
            out.push_str(&plain[refs_plain[i].start..refs_plain[i].end]);
        }
        last = r.end;
    }
    out.push_str(&swapped[last..]);
    out
}

/// Updates references after rows or columns are inserted (`count > 0`) or deleted
/// (`count < 0`) at `at` on `target_sheet`. `own_sheet` is the sheet that holds the
/// formula (references without a sheet prefix point there).
pub fn shift_refs(formula: &str, own_sheet: &str, target_sheet: &str, axis: Axis, at: i32, count: i32) -> String {
    rewrite(formula, |r| {
        let sheet = r.sheet.as_deref().unwrap_or(own_sheet);
        if !sheet.eq_ignore_ascii_case(target_sheet) || count == 0 {
            return None;
        }
        let get = |c: &Corner| match axis {
            Axis::Row => c.row,
            Axis::Column => c.col,
        };
        let set = |c: &mut Corner, v: Option<(i32, bool)>| match axis {
            Axis::Row => c.row = v,
            Axis::Column => c.col = v,
        };
        let mut a = r.a;
        let mut b = r.b;
        if count > 0 {
            for c in std::iter::once(&mut a).chain(b.iter_mut()) {
                if let Some((v, abs)) = get(c) {
                    if v >= at {
                        set(c, Some((v + count, abs)));
                    }
                }
            }
            return Some(render(r, a, b));
        }
        let n = -count;
        let del_end = at + n - 1;
        let shift = |v: i32| if v > del_end { v - n } else { v };
        match b {
            None => {
                if let Some((v, abs)) = get(&a) {
                    if (at..=del_end).contains(&v) {
                        return Some("#REF!".to_string());
                    }
                    set(&mut a, Some((shift(v), abs)));
                }
                Some(render(r, a, None))
            }
            Some(mut bb) => {
                let (Some((v0, abs0)), Some((v1, abs1))) = (get(&a), get(&bb)) else {
                    return None; // whole rows/columns along the other axis
                };
                let (lo, hi) = (v0.min(v1), v0.max(v1));
                if lo >= at && hi <= del_end {
                    return Some("#REF!".to_string());
                }
                let new_lo = if lo >= at && lo <= del_end { at } else { shift(lo) };
                let new_hi = if hi >= at && hi <= del_end { at - 1 } else { shift(hi) };
                set(&mut a, Some((new_lo, abs0)));
                set(&mut bb, Some((new_hi, abs1)));
                Some(render(r, a, Some(bb)))
            }
        }
    })
}

/// Toggles the reference that ends at (or contains) byte `cursor` through Excel's F4 cycle:
/// A1 → $A$1 → A$1 → $A1 → A1. Returns the new text and the new cursor position.
pub fn cycle_absolute(formula: &str, cursor: usize) -> Option<(String, usize)> {
    let refs = find_refs(formula);
    let r = refs.iter().find(|r| cursor >= r.start && cursor <= r.end)?;
    let next = |c: Corner| -> Corner {
        let state = (c.col.map(|x| x.1).unwrap_or(false), c.row.map(|x| x.1).unwrap_or(false));
        let (ca, ra) = match state {
            (false, false) => (true, true),
            (true, true) => (false, true),
            (false, true) => (true, false),
            (true, false) => (false, false),
        };
        Corner { col: c.col.map(|(v, _)| (v, ca)), row: c.row.map(|(v, _)| (v, ra)) }
    };
    let text = render(r, next(r.a), r.b.map(next));
    let mut out = String::new();
    out.push_str(&formula[..r.start]);
    out.push_str(&text);
    let new_cursor = out.len();
    out.push_str(&formula[r.end..]);
    Some((out, new_cursor))
}

/// Range references in a formula, for highlighting while editing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RefSpan {
    pub start: usize,
    pub end: usize,
    pub sheet: Option<String>,
    pub r0: i32,
    pub c0: i32,
    pub r1: i32,
    pub c1: i32,
}

pub fn ref_spans(formula: &str) -> Vec<RefSpan> {
    find_refs(formula)
        .into_iter()
        .map(|r| {
            let b = r.b.unwrap_or(r.a);
            let c0 = r.a.col.map(|x| x.0).unwrap_or(1);
            let c1 = b.col.map(|x| x.0).unwrap_or(LAST_COL);
            let r0 = r.a.row.map(|x| x.0).unwrap_or(1);
            let r1 = b.row.map(|x| x.0).unwrap_or(LAST_ROW);
            RefSpan {
                start: r.start,
                end: r.end,
                sheet: r.sheet.clone(),
                r0: r0.min(r1),
                c0: c0.min(c1),
                r1: r0.max(r1),
                c1: c0.max(c1),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transposed_paste_swaps_relative_offsets() {
        // B1 = A1*10 pasted transposed at C4 (from C3:C4) refers to C3.
        assert_eq!(transpose_refs("=A1*10", (1, 2), (4, 3)), "=C3*10");
        assert_eq!(transpose_refs("=$A$1+A1", (1, 2), (4, 3)), "=$A$1+C3");
    }

    #[test]
    fn copies_move_relative_parts_only() {
        assert_eq!(
            offset_refs("=SUMIFS(t[a], t[b], A5, t[c], \">=\"&$H$4)", 1, 0),
            "=SUMIFS(t[a], t[b], A6, t[c], \">=\"&$H$4)"
        );
        assert_eq!(offset_refs("=A$1+$A1+B2:C3", 2, 1), "=B$1+$A3+C4:D5");
        assert_eq!(offset_refs("=A1", -1, 0), "=#REF!");
        // Strings, function names and structured references are left alone.
        assert_eq!(offset_refs("=LOG10(A1)&\"A1\"&t[A1]", 1, 0), "=LOG10(A2)&\"A1\"&t[A1]");
        assert_eq!(offset_refs("=Sheet2!A1+'My Sheet'!B2", 1, 0), "=Sheet2!A2+'My Sheet'!B3");
    }

    #[test]
    fn inserted_rows_push_references_down() {
        let f = "=SUMIFS(t[a], t[b], A5) + A2 + SUM(A3:A9)";
        assert_eq!(
            shift_refs(f, "Sheet1", "Sheet1", Axis::Row, 4, 2),
            "=SUMIFS(t[a], t[b], A7) + A2 + SUM(A3:A11)"
        );
        // Other sheets are not affected.
        assert_eq!(shift_refs(f, "Sheet1", "Sheet2", Axis::Row, 4, 2), f);
        assert_eq!(shift_refs("=Sheet2!B5", "Sheet1", "Sheet2", Axis::Row, 1, 1), "=Sheet2!B6");
    }

    #[test]
    fn deleted_rows_and_columns() {
        assert_eq!(shift_refs("=A5+A9", "S", "S", Axis::Row, 5, -2), "=#REF!+A7");
        assert_eq!(shift_refs("=SUM(A3:A9)", "S", "S", Axis::Row, 5, -2), "=SUM(A3:A7)");
        assert_eq!(shift_refs("=SUM(A5:A6)", "S", "S", Axis::Row, 5, -2), "=SUM(#REF!)");
        assert_eq!(shift_refs("=D1", "S", "S", Axis::Column, 2, -1), "=C1");
        assert_eq!(shift_refs("=$D$1", "S", "S", Axis::Column, 2, 1), "=$E$1");
    }

    #[test]
    fn f4_cycles_like_excel() {
        let (t, c) = cycle_absolute("=SUM(A1", 7).unwrap();
        assert_eq!((t.as_str(), c), ("=SUM($A$1", 9));
        let (t, _) = cycle_absolute(&t, 9).unwrap();
        assert_eq!(t, "=SUM(A$1");
        let (t, _) = cycle_absolute(&t, 8).unwrap();
        assert_eq!(t, "=SUM($A1");
        let (t, _) = cycle_absolute(&t, 8).unwrap();
        assert_eq!(t, "=SUM(A1");
        let (t, _) = cycle_absolute("=A1:B2+1", 3).unwrap();
        assert_eq!(t, "=$A$1:$B$2+1");
    }

    #[test]
    fn spans_for_highlighting() {
        let s = ref_spans("=A1+SUM(B2:C3)+A:A");
        assert_eq!(s.len(), 3);
        assert_eq!((s[1].r0, s[1].c0, s[1].r1, s[1].c1), (2, 2, 3, 3));
        assert_eq!((s[2].r0, s[2].r1), (1, LAST_ROW));
    }
}
