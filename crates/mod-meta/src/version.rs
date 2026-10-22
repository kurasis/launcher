//! Versions compared and tested against ranges as the loaders do: Maven's
//! ranges for Forge and NeoForge (`[1.21.1,)`, `[47,48)`), Fabric's
//! predicates for Fabric and Quilt (`>=0.16 <1`, `~1.21`, `1.21.x`).

use std::cmp::Ordering;

use crate::Loader;

/// Longest version and range read; longer ones are not compared.
const MAX_VERSION: usize = 128;
const MAX_RANGE: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Num(u64),
    Word(String),
}

/// Where a word stands against a release (Maven's order, with Minecraft's
/// `pre` before `rc`): before it for pre-releases, after it for anything else.
fn rank(word: &str) -> u8 {
    match word {
        "alpha" | "a" => 0,
        "beta" | "b" => 1,
        "milestone" | "m" => 2,
        "pre" | "preview" => 3,
        "rc" | "cr" => 4,
        "snapshot" => 5,
        "" | "ga" | "final" | "release" => 6,
        "sp" => 7,
        _ => 8,
    }
}

/// A version cut into numbers and words; build metadata after `+` does not
/// count. `None` for what is no version.
fn parts(version: &str) -> Option<Vec<Part>> {
    let version = version.trim();
    if version.is_empty()
        || version.len() > MAX_VERSION
        || version.contains("${")
        || !version.bytes().all(|b| b.is_ascii_graphic())
    {
        return None;
    }
    let version = version.split('+').next().unwrap_or_default();
    let version = version
        .strip_prefix(['v', 'V'])
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or(version);
    let mut out = Vec::new();
    for piece in version.split(['.', '-', '_']) {
        let mut rest = piece;
        while !rest.is_empty() {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            let (run, after) = if digits > 0 {
                rest.split_at(digits)
            } else {
                rest.split_at(rest.bytes().take_while(|b| !b.is_ascii_digit()).count())
            };
            out.push(match run.parse() {
                Ok(n) if digits > 0 => Part::Num(n),
                _ => Part::Word(run.to_ascii_lowercase()),
            });
            rest = after;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn compare_parts(a: &Part, b: &Part) -> Ordering {
    match (a, b) {
        (Part::Num(x), Part::Num(y)) => x.cmp(y),
        (Part::Num(_), Part::Word(_)) => Ordering::Greater,
        (Part::Word(_), Part::Num(_)) => Ordering::Less,
        (Part::Word(x), Part::Word(y)) => match (rank(x), rank(y)) {
            (8, 8) => x.cmp(y),
            (r, s) => r.cmp(&s),
        },
    }
}

/// What a missing part counts as beside `other`: 0 beside a number, a
/// release beside a word (`1.21` = `1.21.0`, `1.21-pre1` < `1.21`).
fn padding(other: &Part) -> Part {
    match other {
        Part::Num(_) => Part::Num(0),
        Part::Word(_) => Part::Word(String::new()),
    }
}

/// Which of two versions is newer; `None` when they cannot be compared (not
/// versions, or one starts with a word and the other with a number).
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    let (a, b) = (parts(a)?, parts(b)?);
    if matches!(a[0], Part::Word(_)) != matches!(b[0], Part::Word(_)) {
        return None;
    }
    for i in 0..a.len().max(b.len()) {
        let order = match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => compare_parts(x, y),
            (Some(x), None) => compare_parts(x, &padding(x)),
            (None, Some(y)) => compare_parts(&padding(y), y),
            (None, None) => Ordering::Equal,
        };
        if order != Ordering::Equal {
            return Some(order);
        }
    }
    Some(Ordering::Equal)
}

/// Whether `version` is in any of `ranges`, written for `loader`; no ranges:
/// any version. `None` when that cannot be told.
pub fn allows(loader: Loader, ranges: &[String], version: &str) -> Option<bool> {
    if ranges.is_empty() {
        return Some(true);
    }
    let mut known = true;
    for range in ranges {
        let answer = if range.len() > MAX_RANGE {
            None
        } else if loader.maven() {
            maven(range, version)
        } else {
            fabric(range, version)
        };
        match answer {
            Some(true) => return Some(true),
            Some(false) => {}
            None => known = false,
        }
    }
    known.then_some(false)
}

// ----- Maven -----

/// A Maven range: restrictions joined by commas (`[1.0,2.0),[3.0,)`), `[a]`
/// for exactly one; a bare version only recommends itself and takes any.
fn maven(spec: &str, version: &str) -> Option<bool> {
    let spec: String = spec.chars().filter(|c| !c.is_whitespace()).collect();
    if spec.is_empty() || spec == "*" || !spec.starts_with(['[', '(']) {
        return Some(true);
    }
    let mut known = true;
    let mut rest = spec.as_str();
    while !rest.is_empty() {
        if !rest.starts_with(['[', '(']) {
            return None;
        }
        let close = rest.find([']', ')'])?;
        let from = rest.starts_with('[');
        let to = rest.as_bytes()[close] == b']';
        match restriction(&rest[1..close], from, to, version) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => known = false,
        }
        rest = &rest[close + 1..];
        rest = rest.strip_prefix(',').unwrap_or(rest);
    }
    known.then_some(false)
}

fn restriction(body: &str, from: bool, to: bool, version: &str) -> Option<bool> {
    let Some((low, high)) = body.split_once(',') else {
        return Some(compare(version, body)? == Ordering::Equal);
    };
    if !low.is_empty() {
        let order = compare(version, low)?;
        if order == Ordering::Less || (order == Ordering::Equal && !from) {
            return Some(false);
        }
    }
    if !high.is_empty() {
        let order = compare(version, high)?;
        if order == Ordering::Greater || (order == Ordering::Equal && !to) {
            return Some(false);
        }
    }
    Some(true)
}

// ----- Fabric -----

/// A Fabric predicate: constraints joined by spaces, all of which must hold.
fn fabric(predicate: &str, version: &str) -> Option<bool> {
    let mut known = true;
    for constraint in predicate.split_whitespace() {
        match fabric_one(constraint, version) {
            Some(true) => {}
            Some(false) => return Some(false),
            None => known = false,
        }
    }
    known.then_some(true)
}

/// The numbers a version starts with (`1.21.1-pre2` → 1, 21, 1).
fn numbers(version: &str) -> Vec<u64> {
    let mut out = Vec::new();
    for piece in version
        .split(['+', '-'])
        .next()
        .unwrap_or_default()
        .split('.')
    {
        let digits: String = piece.chars().take_while(char::is_ascii_digit).collect();
        match digits.parse() {
            Ok(n) => out.push(n),
            Err(_) => break,
        }
        if digits.len() != piece.len() {
            break;
        }
    }
    out
}

/// The version after the `at`-th number goes one up (`1.21.4`, 1 → `1.22`).
fn bump(numbers: &[u64], at: usize) -> Option<String> {
    let mut kept: Vec<u64> = numbers.get(..=at)?.to_vec();
    let last = kept.last_mut()?;
    *last = last.checked_add(1)?;
    Some(
        kept.iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("."),
    )
}

fn at_least(version: &str, low: &str) -> Option<bool> {
    Some(compare(version, low)? != Ordering::Less)
}

fn below(version: &str, high: &str) -> Option<bool> {
    Some(compare(version, high)? == Ordering::Less)
}

fn between(version: &str, low: &str, high: &str) -> Option<bool> {
    Some(at_least(version, low)? && below(version, high)?)
}

fn fabric_one(constraint: &str, version: &str) -> Option<bool> {
    if constraint == "*" {
        return Some(true);
    }
    let (op, bound) = [">=", "<=", ">", "<", "=", "~", "^"]
        .iter()
        .find_map(|op| constraint.strip_prefix(op).map(|b| (*op, b)))
        .unwrap_or(("", constraint));
    // `1.21.x`: the numbers before the x are fixed.
    let pieces: Vec<&str> = bound.split('.').collect();
    if let Some(wild) = pieces.iter().position(|p| matches!(*p, "x" | "X" | "*")) {
        if wild == 0 {
            return Some(true);
        }
        let fixed = pieces[..wild].join(".");
        let high = bump(&numbers(&fixed), wild - 1)?;
        return match op {
            ">=" => at_least(version, &fixed),
            ">" => at_least(version, &high),
            "<" => below(version, &fixed),
            "<=" => below(version, &high),
            _ => between(version, &fixed, &high),
        };
    }
    match op {
        ">=" => at_least(version, bound),
        "<=" => Some(compare(version, bound)? != Ordering::Greater),
        ">" => Some(compare(version, bound)? == Ordering::Greater),
        "<" => below(version, bound),
        // ~1.21.1: the same 1.21; ~1: the same 1.
        "~" => {
            let numbers = numbers(bound);
            let at = if numbers.len() >= 2 { 1 } else { 0 };
            between(version, bound, &bump(&numbers, at)?)
        }
        // ^1.2.3: the same major version.
        "^" => between(version, bound, &bump(&numbers(bound), 0)?),
        _ => Some(compare(version, bound)? == Ordering::Equal),
    }
}
