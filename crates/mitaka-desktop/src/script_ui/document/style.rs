//! 静的文書で解釈する CSS。未知の表示規則がある文書は読み取り専用とする。
use cssparser::{
    AtRuleParser, CowRcStr, DeclarationParser, ParseError, Parser, ParserInput, ParserState,
    QualifiedRuleParser, RuleBodyItemParser, RuleBodyParser, StyleSheetParser,
};
use eframe::egui::Color32;
use scraper::{ElementRef, Selector};

#[derive(Clone, Copy, Default)]
pub(super) struct Paint {
    pub fg: Option<Color32>,
    pub bg: Option<Color32>,
    pub size: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub hidden: bool,
}
#[derive(Default, Clone)]
struct Patch {
    fg: Option<Color32>,
    bg: Option<Color32>,
    size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    hidden: bool,
}
impl Patch {
    fn apply(&self, v: &mut Paint) {
        if let Some(c) = self.fg {
            v.fg = Some(c)
        }
        if let Some(c) = self.bg {
            v.bg = Some(c)
        }
        if let Some(sz) = self.size {
            v.size = Some(sz)
        }
        if let Some(b) = self.bold {
            v.bold = b
        }
        if let Some(b) = self.italic {
            v.italic = b
        }
        if let Some(b) = self.underline {
            v.underline = b
        }
        v.hidden |= self.hidden;
    }
    fn set(&mut self, key: &str, raw: &str) -> bool {
        let v = raw
            .trim()
            .trim_end_matches("!important")
            .trim()
            .to_ascii_lowercase();
        if raw.contains('!') {
            return false;
        }
        match key.to_ascii_lowercase().as_str() {
            "display" if v == "none" => self.hidden = true,
            "display"
                if [
                    "inline",
                    "block",
                    "inline-block",
                    "table",
                    "table-row",
                    "table-cell",
                    "list-item",
                ]
                .contains(&v.as_str()) => {}
            "visibility" if matches!(v.as_str(), "hidden" | "collapse") => self.hidden = true,
            "visibility" if v == "visible" => {}
            "opacity" => match v.parse::<f32>() {
                Ok(n) if (0.0..=1.).contains(&n) => self.hidden |= n < 1.,
                _ => return false,
            },
            "color" if v == "transparent" => self.hidden = true,
            "color" => match color(&v) {
                Some(c) => self.fg = Some(c),
                None => return false,
            },
            "background-color" | "background" => {
                if v == "transparent" || v == "none" {
                    return true;
                }
                if let Some(c) = v.split_ascii_whitespace().next().and_then(color) {
                    self.bg = Some(c);
                }
            }
            "font-weight" => {
                self.bold = Some(matches!(
                    v.as_str(),
                    "bold" | "bolder" | "600" | "700" | "800" | "900"
                ))
            }
            "font-style" => self.italic = Some(matches!(v.as_str(), "italic" | "oblique")),
            "text-decoration" => self.underline = Some(v.contains("underline")),
            "font-size" => {
                let px = if v == "medium" {
                    Some(16.)
                } else if v == "small" {
                    Some(13.)
                } else if v == "large" {
                    Some(20.)
                } else if let Some(p) = v.strip_suffix("px") {
                    p.parse().ok()
                } else if let Some(p) = v.strip_suffix('%') {
                    p.parse::<f32>().ok().map(|s| s * 0.16)
                } else {
                    v.parse().ok()
                };
                let Some(n) = px.filter(|n| n.is_finite() && *n >= 0.) else {
                    return false;
                };
                self.hidden |= n < 5.;
                self.size = Some(n.clamp(5., 72.));
            }
            "width" | "height" if v == "auto" => {}
            "width" | "height" => {
                if metrics(&v).is_none() {
                    return false;
                }
                self.hidden |= v
                    .split_ascii_whitespace()
                    .any(|a| metric(a).is_some_and(|n| n == 0.));
            }
            "font-family" | "clear" | "background-repeat" | "list-style-image"
            | "list-style-type" | "text-align" => {}
            k if k == "padding"
                || k.starts_with("padding-")
                || k == "margin"
                || k.starts_with("margin-")
                || k == "letter-spacing" =>
            {
                if metrics(&v).is_none() {
                    return false;
                }
            }
            k if k == "border" || k.starts_with("border-") => {}
            _ => return false,
        }
        true
    }
}
fn metric(v: &str) -> Option<f32> {
    let n = v
        .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%')
        .parse::<f32>()
        .ok()?;
    (n.is_finite() && (0.0..=1024.).contains(&n)).then_some(n)
}
fn metrics(v: &str) -> Option<()> {
    (!v.is_empty()
        && v.split_ascii_whitespace()
            .all(|s| s == "auto" || metric(s).is_some()))
    .then_some(())
}
fn color(v: &str) -> Option<Color32> {
    let s = v.strip_prefix('#');
    let n = match s {
        Some(s) if s.len() == 3 => {
            let v = u16::from_str_radix(s, 16).ok()?;
            return Some(Color32::from_rgb(
                ((v >> 8) & 15) as u8 * 17,
                ((v >> 4) & 15) as u8 * 17,
                (v & 15) as u8 * 17,
            ));
        }
        Some(s) if s.len() == 6 => u32::from_str_radix(s, 16).ok()?,
        None => match v {
            "black" => 0,
            "white" => 0xffffff,
            "red" => 0xff0000,
            "green" => 0x008000,
            "blue" => 0x0000ff,
            "yellow" => 0xffff00,
            "gray" | "grey" => 0x808080,
            _ => return None,
        },
        _ => return None,
    };
    Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
fn rest<'a>(p: &mut Parser<'a, '_>) -> &'a str {
    let start = p.position();
    while p.next_including_whitespace_and_comments().is_ok() {}
    p.slice_from(start)
}
struct Props;
impl<'i> DeclarationParser<'i> for Props {
    type Declaration = (String, String);
    type Error = ();
    fn parse_value<'t>(
        &mut self,
        name: CowRcStr<'i>,
        p: &mut Parser<'i, 't>,
        _: &ParserState,
    ) -> Result<Self::Declaration, ParseError<'i, ()>> {
        Ok((name.to_string(), rest(p).into()))
    }
}
impl<'i> AtRuleParser<'i> for Props {
    type Prelude = ();
    type AtRule = (String, String);
    type Error = ();
}
impl<'i> QualifiedRuleParser<'i> for Props {
    type Prelude = ();
    type QualifiedRule = (String, String);
    type Error = ();
}
impl<'i> RuleBodyItemParser<'i, (String, String), ()> for Props {
    fn parse_declarations(&self) -> bool {
        true
    }
    fn parse_qualified(&self) -> bool {
        false
    }
}
fn declarations(p: &mut Parser<'_, '_>) -> (Patch, bool) {
    let mut patch = Patch::default();
    let mut safe = true;
    for item in RuleBodyParser::new(p, &mut Props) {
        match item {
            Ok((k, v)) => safe &= patch.set(&k, &v),
            // Plus 1.5.4 の base.css の旧 IE 用無効宣言。CSS の at-rule としても表示効果を持たない。
            Err((_, text))
                if text
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("@padding:") => {}
            Err(_) => safe = false,
        }
    }
    (patch, safe)
}
struct Sheet;
impl<'i> AtRuleParser<'i> for Sheet {
    type Prelude = ();
    type AtRule = (String, Patch, bool);
    type Error = ();
}
impl<'i> QualifiedRuleParser<'i> for Sheet {
    type Prelude = String;
    type QualifiedRule = (String, Patch, bool);
    type Error = ();
    fn parse_prelude<'t>(&mut self, p: &mut Parser<'i, 't>) -> Result<String, ParseError<'i, ()>> {
        Ok(rest(p).trim().into())
    }
    fn parse_block<'t>(
        &mut self,
        name: String,
        _: &ParserState,
        p: &mut Parser<'i, 't>,
    ) -> Result<Self::QualifiedRule, ParseError<'i, ()>> {
        let (patch, safe) = declarations(p);
        Ok((name, patch, safe))
    }
}
#[derive(Default)]
pub(super) struct Styles {
    rules: Vec<(u32, Selector, Patch)>,
    interactive: Vec<(Selector, Patch)>,
    pub blocked: bool,
}
impl Styles {
    pub fn sheet(&mut self, css: &str) {
        let mut input = ParserInput::new(css);
        let mut p = Parser::new(&mut input);
        for row in StyleSheetParser::new(&mut p, &mut Sheet) {
            let Ok((selectors, style, safe)) = row else {
                self.blocked = true;
                continue;
            };
            self.blocked |= !safe;
            for sel in selectors.split(',').map(str::trim) {
                if self.rules.len() + self.interactive.len() >= 256 || sel.len() > 128 {
                    self.blocked = true;
                    return;
                }
                if sel.contains(':')
                    && sel
                        .split(':')
                        .skip(1)
                        .all(|s| s == "visited" || s == "hover")
                {
                    // 擬似状態で表示可否が変わるリンクを、常時実行可にしない。
                    self.blocked |= style.hidden || style.size.is_some() || style.bg.is_some();
                    let base = sel.split(':').next().unwrap_or_default();
                    if !base.is_empty()
                        && base.len() <= 128
                        && base
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-# ".contains(&b))
                    {
                        if let Ok(selector) = Selector::parse(base) {
                            self.interactive.push((selector, style.clone()));
                        } else {
                            self.blocked = true;
                        }
                    } else {
                        self.blocked = true;
                    }
                    continue;
                }
                if !sel
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_- .#*>".contains(&c))
                {
                    self.blocked = true;
                    continue;
                }
                if let Ok(selector) = Selector::parse(sel) {
                    let ids = sel.bytes().filter(|&b| b == b'#').count() as u32;
                    let classes = sel.bytes().filter(|&b| b == b'.').count() as u32;
                    let tags = sel
                        .split([' ', '>'])
                        .filter(|t| t.as_bytes().first().is_some_and(u8::is_ascii_alphabetic))
                        .count() as u32;
                    self.rules
                        .push((ids * 65536 + classes * 256 + tags, selector, style.clone()));
                } else {
                    self.blocked = true;
                }
            }
        }
    }
    pub fn finish(&mut self) {
        self.rules.sort_by_key(|r| r.0);
    }
    pub fn paint(&self, e: ElementRef<'_>, mut parent: Paint) -> (Paint, bool) {
        for (_, selector, rule) in &self.rules {
            if selector.matches(&e) {
                rule.apply(&mut parent)
            }
        }
        let mut safe = true;
        if let Some(inline) = e.value().attr("style") {
            let mut input = ParserInput::new(inline);
            let (r, ok) = declarations(&mut Parser::new(&mut input));
            r.apply(&mut parent);
            safe = ok;
        }
        if e.value().name() == "a" {
            for (selector, rule) in &self.interactive {
                if selector.matches(&e) {
                    let mut variant = parent;
                    rule.apply(&mut variant);
                    safe &= variant.legible();
                }
            }
        }
        (parent, safe)
    }
}

impl Paint {
    pub fn legible(&self) -> bool {
        let Some((text, bg)) = self.fg.zip(self.bg) else {
            return true;
        };
        let lum = |c: Color32| {
            let energy = |v: u8| {
                let x = f32::from(v) / 255.;
                if x <= 0.04045 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * energy(c.r()) + 0.7152 * energy(c.g()) + 0.0722 * energy(c.b())
        };
        let (a, b) = (lum(text), lum(bg));
        (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 1.5
    }
    pub fn rich(&self, text: &str, heading: u8, link: bool) -> eframe::egui::RichText {
        let base = match heading {
            1 => 30.,
            2 => 24.,
            3 => 20.,
            4 => 17.,
            _ => 15.,
        };
        let mut out = eframe::egui::RichText::new(text).size(self.size.unwrap_or(base));
        if let Some(mut c) = self.fg {
            if !self.legible() {
                let bg = self.bg.unwrap_or(Color32::WHITE);
                c = if bg.r().max(bg.g()).max(bg.b()) > 140 {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
            }
            out = out.color(c)
        }
        if let Some(c) = self.bg {
            out = out.background_color(c)
        }
        if self.bold {
            out = out.strong()
        }
        if self.italic {
            out = out.italics()
        }
        if self.underline || link {
            out = out.underline()
        }
        out
    }
}
