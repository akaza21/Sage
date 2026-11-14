//! Port of node-semver 7.7.4 internal/re.js and classes/range.js (loose=false,
//! includePrerelease=false). Keep grammar and replacement order together: early
//! cleanup changes which malformed ranges npm accepts. Only the final comparison
//! is specialized for release runtimes; prerelease bounds still sort below them.

// node-semver: Copyright (c) Isaac Z. Schlueter and Contributors (ISC).
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR
// IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::{Excluded, Included, Interval, NodeVersion, Unbounded, number};

const NPM_WHITESPACE: &str = concat!(
    "\t\n\u{b}\u{c}\r \u{a0}\u{1680}",
    "\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}",
    "\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}",
    "\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}",
);

pub(super) fn npm_whitespace(character: char) -> bool {
    NPM_WHITESPACE.contains(character)
}

pub(super) fn normalize_whitespace(text: &str) -> String {
    GRAMMAR.space.replace_all(text, " ").into_owned()
}

struct Grammar {
    space: Regex,
    hyphen: Regex,
    comparator_trim: Regex,
    tilde_trim: Regex,
    caret_trim: Regex,
    build: Regex,
    caret: Regex,
    tilde: Regex,
    xrange: Regex,
    star: Regex,
    gte0: Regex,
    comparator: Regex,
}

static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
    let whitespace = format!("[{NPM_WHITESPACE}]");
    let compile = |pattern: &str| {
        // internal/re.js makeSafeRegex: range.js uses safeRe after collapsing
        // whitespace. BUILD's 250-character bound matters even below 256 bytes.
        let mut pattern = pattern.to_owned();
        for (token, max) in [(r"\s", 1), ("[0-9]", 256), ("[a-zA-Z0-9-]", 250)] {
            pattern = pattern
                .replace(&format!("{token}*"), &format!("{token}{{0,{max}}}"))
                .replace(&format!("{token}+"), &format!("{token}{{1,{max}}}"));
        }
        Regex::new(&pattern.replace(r"\s", &whitespace)).expect("npm range regex")
    };
    let numeric = r"0|[1-9][0-9]*";
    let nonnumeric = r"[0-9]*[a-zA-Z-][a-zA-Z0-9-]*";
    let pre_id = format!("(?:{nonnumeric}|{numeric})");
    let prerelease = format!(r"(?:-({pre_id}(?:\.{pre_id})*))");
    let build = r"(?:\+([a-zA-Z0-9-]+(?:\.[a-zA-Z0-9-]+)*))";
    let full = format!(r"v?({numeric})\.({numeric})\.({numeric}){prerelease}?{build}?");
    // COMPARATORTRIM deliberately uses LOOSEPLAIN even in strict mode.
    let loose_id = format!("(?:{nonnumeric}|[0-9]+)");
    let loose = format!(r"[v=\s]*[0-9]+\.[0-9]+\.[0-9]+(?:-?{loose_id}(?:\.{loose_id})*)?{build}?");
    let x = format!(r"{numeric}|x|X|\*");
    let partial = format!(r"[v=\s]*({x})(?:\.({x})(?:\.({x})(?:{prerelease})?{build}?)?)?");
    let gtlt = r"((?:<|>)?=?)";
    Grammar {
        // Range's constructor uses /\s+/g, not the bounded safeRe grammar.
        space: Regex::new(&format!("{whitespace}+")).expect("npm whitespace regex"),
        hyphen: compile(&format!(r"^\s*({partial})\s+-\s+({partial})\s*$")),
        comparator_trim: compile(&format!(r"(\s*){gtlt}\s*({loose}|{partial})")),
        tilde_trim: compile(r"(\s*)~>?\s+"),
        caret_trim: compile(r"(\s*)\^\s+"),
        build: compile(build),
        caret: compile(&format!(r"^\^{partial}$")),
        tilde: compile(&format!(r"^~>?{partial}$")),
        xrange: compile(&format!(r"^{gtlt}\s*{partial}$")),
        star: compile(r"(<|>)?=?\s*\*"),
        gte0: compile(r"^\s*>=\s*0\.0\.0\s*$"),
        comparator: compile(&format!(r"^{gtlt}\s*({full})$|^$")),
    }
});

pub(super) fn comparator_set(set: &str) -> Option<Vec<Interval>> {
    let g = &*GRAMMAR;
    // Range.parseRange: hyphens BEFORE comparator, tilde and caret trimming.
    let set = match g.hyphen.captures(set) {
        Some(c) => hyphen(&c)?,
        None => set.to_owned(),
    };
    let set = g.comparator_trim.replace_all(&set, "${1}${2}${3}");
    let set = g.tilde_trim.replace_all(&set, "${1}~");
    let set = g.caret_trim.replace_all(&set, "${1}^");
    let mut expanded = Vec::new();
    for comp in set.split(' ') {
        // parseComparator: remove the FIRST build match, then caret, tilde,
        // x-range and star. These are replacements, not eager validation.
        let comp = g.build.replace(comp, "");
        let comp = replace_parts(&comp, &g.caret, |c| compatible(c, true))?;
        let comp = replace_parts(&comp, &g.tilde, |c| compatible(c, false))?;
        let comp = replace_parts(&comp, &g.xrange, xrange)?;
        expanded.push(
            g.star
                .replace(comp.trim_matches(npm_whitespace), "")
                .into_owned(),
        );
    }
    // Validate EVERY comparator before evaluating; even an ANY/null set cannot
    // hide an invalid comparator or an invalid later || alternative.
    expanded
        .join(" ")
        .split(' ')
        .map(|comp| comparator(&g.gte0.replace(comp.trim_matches(npm_whitespace), "")))
        .collect()
}

fn replace_parts(
    text: &str,
    pattern: &Regex,
    replace: impl Fn(&Captures<'_>) -> Option<String>,
) -> Option<String> {
    text.trim_matches(npm_whitespace)
        .split(' ')
        .map(|part| match pattern.captures(part) {
            Some(c) => replace(&c),
            None => Some(part.to_owned()),
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join(" "))
}

fn capture<'a>(captures: &Captures<'a>, index: usize) -> &'a str {
    captures.get(index).map_or("", |m| m.as_str())
}

fn is_x(part: &str) -> bool {
    matches!(part, "" | "x" | "X" | "*")
}

// Preserve raw components until replacement: wildcard branches can discard even
// out-of-range numbers. SemVer's numeric limit applies to final comparators.
struct Partial<'a> {
    parts: [&'a str; 3],
    prerelease: &'a str,
}

impl<'a> Partial<'a> {
    fn new(c: &Captures<'a>, start: usize) -> Self {
        Self {
            parts: std::array::from_fn(|i| capture(c, start + i)),
            prerelease: capture(c, start + 3),
        }
    }

    fn wildcard(&self, index: usize) -> bool {
        self.parts[..=index].iter().any(|part| is_x(part))
    }

    fn floor(&self) -> String {
        (0..3)
            .map(|i| if self.wildcard(i) { "0" } else { self.parts[i] })
            .collect::<Vec<_>>()
            .join(".")
    }

    fn next(&self, index: usize) -> Option<String> {
        let mut parts = self.parts.map(str::to_owned);
        // Generated comparators above MAX_SAFE_INTEGER are invalid in npm too.
        parts[index] = number(self.parts[index])?.checked_add(1)?.to_string();
        parts[index + 1..].fill("0".to_owned());
        Some(parts.join("."))
    }
}

fn compatible(c: &Captures<'_>, caret: bool) -> Option<String> {
    let v = Partial::new(c, 1);
    if v.wildcard(0) {
        return Some(String::new());
    }
    let [major, minor, _] = v.parts;
    let index = if v.wildcard(1) || (caret && major != "0") {
        0
    } else if !caret || v.wildcard(2) || minor != "0" {
        1
    } else {
        2
    };
    let pre = if !v.wildcard(2) && !v.prerelease.is_empty() {
        format!("-{}", v.prerelease)
    } else {
        String::new()
    };
    Some(format!(">={}{pre} <{}-0", v.floor(), v.next(index)?))
}

fn xrange(c: &Captures<'_>) -> Option<String> {
    let v = Partial::new(c, 2);
    let mut operator = capture(c, 1);
    if operator == "=" && v.wildcard(2) {
        operator = "";
    }
    if v.wildcard(0) {
        return Some(
            if matches!(operator, ">" | "<") {
                "<0.0.0-0"
            } else {
                "*"
            }
            .to_owned(),
        );
    }
    if !operator.is_empty() && v.wildcard(2) {
        let floor = if matches!(operator, ">" | "<=") {
            v.next(if v.wildcard(1) { 0 } else { 1 })?
        } else {
            v.floor()
        };
        operator = match operator {
            ">" => ">=",
            "<=" => "<",
            other => other,
        };
        return Some(format!(
            "{operator}{floor}{}",
            if operator == "<" { "-0" } else { "" }
        ));
    }
    if v.wildcard(2) {
        return Some(format!(
            ">={} <{}-0",
            v.floor(),
            v.next(if v.wildcard(1) { 0 } else { 1 })?
        ));
    }
    Some(capture(c, 0).to_owned())
}

fn hyphen(c: &Captures<'_>) -> Option<String> {
    let from = Partial::new(c, 2);
    let to = Partial::new(c, 8);
    let low = if from.wildcard(0) {
        String::new()
    } else if from.wildcard(2) {
        format!(">={}", from.floor())
    } else {
        format!(">={}", capture(c, 1))
    };
    let high = if to.wildcard(0) {
        String::new()
    } else if to.wildcard(2) {
        format!("<{}-0", to.next(if to.wildcard(1) { 0 } else { 1 })?)
    } else if !to.prerelease.is_empty() {
        format!("<={}-{}", to.floor(), to.prerelease)
    } else {
        format!("<={}", capture(c, 7))
    };
    Some(
        format!("{low} {high}")
            .trim_matches(npm_whitespace)
            .to_owned(),
    )
}

fn comparator(text: &str) -> Option<Interval> {
    let c = GRAMMAR.comparator.captures(text)?;
    if text.is_empty() {
        return Some(Interval::ANY);
    }
    let node: NodeVersion = (
        number(capture(&c, 3))?,
        number(capture(&c, 4))?,
        number(capture(&c, 5))?,
    );
    let prerelease = !capture(&c, 6).is_empty();
    Some(match capture(&c, 1) {
        ">=" => Interval {
            low: Included(node),
            high: Unbounded,
        },
        ">" => Interval {
            low: if prerelease {
                Included(node)
            } else {
                Excluded(node)
            },
            high: Unbounded,
        },
        "<" => Interval {
            low: Unbounded,
            high: Excluded(node),
        },
        "<=" => Interval {
            low: Unbounded,
            high: if prerelease {
                Excluded(node)
            } else {
                Included(node)
            },
        },
        _ if prerelease => Interval::NONE,
        _ => Interval {
            low: Included(node),
            high: Included(node),
        },
    })
}
