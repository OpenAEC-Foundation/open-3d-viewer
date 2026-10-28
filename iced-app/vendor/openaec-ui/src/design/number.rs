//! Strict number input (issue #8): one reader for every app, so a number is never
//! read differently than it was typed.
//!
//! The UI research found `1 234,56` read as `1`, `12abc` as `12` and `1,2,3` as
//! `1.2`, and Calc and PDF reading `1.234` differently. Here the whole text has
//! to be a number in the field's [`NumberFormat`], or the field says why not:
//!
//! - With a decimal comma (Dutch), a point or space only separates thousands, in
//!   groups of exactly three: `1.234,5` and `1 234,5` are 1234.5, while `1.5` is
//!   refused with "Gebruik een komma als decimaalteken". With a decimal point the
//!   roles swap.
//! - More decimals than the field allows are refused, never rounded silently.
//! - The unit may be typed after the number, such as `12,5 m²`.
//! - Empty, an unfinished entry (`-`, `1,`), and a number are different states,
//!   and a field can also show "Gemengd" or "Onbekend" ([`Value`]).
use iced::widget::{row, text};
use iced::{Alignment, Element, Length};

use super::widgets::{self as w, Density};
use super::{font, Look};
use crate::fonts;

/// Which sign separates the decimals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decimal {
    /// `1.234,56`, as in Dutch.
    Comma,
    /// `1,234.56`, as in English.
    Point,
}

/// The rules of one number field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberFormat {
    pub decimals: u8,
    pub decimal: Decimal,
    /// Shown after the field and allowed after the typed number.
    pub unit: Option<&'static str>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Whether the field may be left empty.
    pub allow_empty: bool,
}

impl NumberFormat {
    /// A field with this many decimals, a decimal comma and no limits.
    pub const fn new(decimals: u8) -> Self {
        Self { decimals, decimal: Decimal::Comma, unit: None, min: None, max: None, allow_empty: false }
    }
    pub const fn unit(mut self, unit: &'static str) -> Self {
        self.unit = Some(unit);
        self
    }
    pub const fn min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }
    pub const fn max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }
    pub const fn point(mut self) -> Self {
        self.decimal = Decimal::Point;
        self
    }
    pub const fn optional(mut self) -> Self {
        self.allow_empty = true;
        self
    }
}

/// What a field holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Empty,
    Number(f64),
    /// A selection of several objects with different values.
    Mixed,
    /// Not known yet, such as a result that is not calculated.
    Unknown,
}

/// The result of reading a text.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Empty,
    /// Still being typed, such as `-` or `1,`: no error yet, no value either.
    Incomplete,
    Number(f64),
    /// Not a number in this format, with the reason for the person typing.
    Invalid(String),
}

/// Writes a number in the field's format, with thousands separators.
pub fn format(value: f64, f: &NumberFormat) -> String {
    let (decimal, group) = match f.decimal {
        Decimal::Comma => (',', '.'),
        Decimal::Point => ('.', ','),
    };
    let fixed = format!("{:.*}", usize::from(f.decimals), value.abs());
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(group);
        }
        grouped.push(ch);
    }
    let sign = if value < 0.0 && fixed.chars().any(|c| c != '0' && c != '.') { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}{decimal}{frac}") }
}

/// Reads a text in the field's format. The whole text must be the number.
pub fn parse(input: &str, f: &NumberFormat) -> Parsed {
    let (decimal, group, decimal_name) = match f.decimal {
        Decimal::Comma => (',', '.', "komma"),
        Decimal::Point => ('.', ',', "punt"),
    };
    let mut body = input.trim();
    if let Some(unit) = f.unit {
        if let Some(rest) = strip_suffix_ignore_case(body, unit) {
            body = rest.trim_end();
        }
    }
    let (negative, body) = match body.chars().next() {
        Some('-' | '−') => (true, body[body.chars().next().map_or(0, char::len_utf8)..].trim_start()),
        Some('+') => (false, body[1..].trim_start()),
        _ => (false, body),
    };
    if body.is_empty() {
        return if input.trim().is_empty() { Parsed::Empty } else { Parsed::Incomplete };
    }
    if let Some(bad) = body.chars().find(|c| !(c.is_ascii_digit() || *c == decimal || *c == group || *c == ' ' || *c == '\u{a0}')) {
        return Parsed::Invalid(format!("“{bad}” hoort niet in een getal."));
    }
    if body.matches(decimal).count() > 1 {
        return Parsed::Invalid(format!("Gebruik maar één {decimal_name}."));
    }
    let (int, frac) = match body.split_once(decimal) {
        Some((int, frac)) => (int, Some(frac)),
        None => (body, None),
    };
    if let Some(frac) = frac {
        if f.decimals == 0 {
            return Parsed::Invalid("Alleen hele getallen.".into());
        }
        if frac.is_empty() {
            return Parsed::Incomplete;
        }
        if !frac.chars().all(|c| c.is_ascii_digit()) {
            return Parsed::Invalid("Na het decimaalteken alleen cijfers.".into());
        }
        if frac.len() > usize::from(f.decimals) {
            return Parsed::Invalid(match f.decimals {
                1 => "Gebruik ten hoogste 1 decimaal.".into(),
                n => format!("Gebruik ten hoogste {n} decimalen."),
            });
        }
    }
    // Thousands: a group sign or space between groups of exactly three digits.
    let groups: Vec<&str> = int.split(|c| c == group || c == ' ' || c == '\u{a0}').collect();
    if groups.len() > 1 {
        let ok = !groups[0].is_empty() && groups[0].len() <= 3 && groups[1..].iter().all(|g| g.len() == 3);
        if !ok {
            return Parsed::Invalid(if int.contains(group) {
                format!("Gebruik een {decimal_name} als decimaalteken; de {} scheidt alleen duizendtallen.", if group == '.' { "punt" } else { "komma" })
            } else {
                "Spaties alleen tussen duizendtallen.".into()
            });
        }
    }
    let digits: String = groups.concat();
    let text = format!("{}.{}", if digits.is_empty() { "0" } else { &digits }, frac.unwrap_or("0"));
    let Ok(mut value) = text.parse::<f64>() else {
        return Parsed::Invalid("Geen getal.".into());
    };
    if negative {
        value = -value;
    }
    if let Some(min) = f.min.filter(|min| value < *min) {
        return Parsed::Invalid(format!("Minimaal {}.", format(min, f)));
    }
    if let Some(max) = f.max.filter(|max| value > *max) {
        return Parsed::Invalid(format!("Maximaal {}.", format(max, f)));
    }
    Parsed::Number(value)
}

fn strip_suffix_ignore_case<'s>(text: &'s str, suffix: &str) -> Option<&'s str> {
    let cut = text.len().checked_sub(suffix.len())?;
    (text.is_char_boundary(cut) && text[cut..].eq_ignore_ascii_case(suffix)).then(|| &text[..cut])
}

/// The draft text of a number field and its error. The text stays as typed, also
/// when it is not a valid number yet, so nothing typed is lost.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NumberInput {
    text: String,
    error: Option<String>,
}

impl NumberInput {
    /// A field that shows `value`.
    pub fn new(value: Value, f: &NumberFormat) -> Self {
        let mut input = Self::default();
        input.show(value, f);
        input
    }

    /// Shows `value`, for example after another object is selected or after undo.
    /// The draft text is replaced; any error goes away.
    pub fn show(&mut self, value: Value, f: &NumberFormat) {
        self.text = match value {
            Value::Number(n) => format(n, f),
            _ => String::new(),
        };
        self.error = None;
    }

    /// Takes a new text from the field. Returns the value to store when the whole
    /// text is a valid number (or empty where allowed); otherwise the text is kept
    /// as a draft with its error, and nothing is stored.
    pub fn edit(&mut self, text: String, f: &NumberFormat) -> Option<Value> {
        let parsed = parse(&text, f);
        self.text = text;
        let (error, value) = match parsed {
            Parsed::Number(n) => (None, Some(Value::Number(n))),
            Parsed::Empty if f.allow_empty => (None, Some(Value::Empty)),
            Parsed::Empty => (Some("Vul een getal in.".to_string()), None),
            Parsed::Incomplete => (None, None),
            Parsed::Invalid(why) => (Some(why), None),
        };
        self.error = error;
        value
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// A number field with its unit after it and the error below it. `value` is
/// what the object holds: Mixed and Unknown show as a placeholder.
pub fn number_field<'a, M: Clone + 'a>(
    look: Look,
    input: &'a NumberInput,
    value: Value,
    f: &NumberFormat,
    on_input: Option<impl Fn(String) -> M + 'a>,
    density: Density,
) -> Element<'a, M> {
    let placeholder = match value {
        Value::Mixed => "Gemengd",
        Value::Unknown => "Onbekend",
        Value::Empty if f.allow_empty => "Leeg",
        _ => "",
    };
    let field = w::text_field_in(look, placeholder, input.text(), on_input, density, input.error(), fonts::NUMBERS);
    match f.unit {
        Some(unit) => row![
            iced::widget::container(field).width(Length::Fill),
            text(unit).size(font::SIZE_BODY).font(fonts::MONO).color(look.color.text_secondary).width(Length::Shrink),
        ]
        .spacing(6)
        .align_y(Alignment::Start)
        .into(),
        None => field,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EURO: NumberFormat = NumberFormat::new(2);
    const COUNT: NumberFormat = NumberFormat::new(0).min(0.0);

    fn number(text: &str, f: &NumberFormat) -> Option<f64> {
        match parse(text, f) {
            Parsed::Number(n) => Some(n),
            _ => None,
        }
    }

    #[test]
    fn the_cases_from_the_ui_research_are_read_or_refused_as_typed() {
        // UI-012: these were read as 1, 12 and 1.2.
        assert_eq!(number("1 234,56", &EURO), Some(1234.56));
        assert!(matches!(parse("12abc", &EURO), Parsed::Invalid(_)));
        assert!(matches!(parse("1,2,3", &EURO), Parsed::Invalid(_)));
        // UI-015: a point is a thousands separator in Dutch, never a decimal sign.
        assert_eq!(number("1.234", &EURO), Some(1234.0));
        assert_eq!(number("1.234.567,89", &EURO), Some(1_234_567.89));
        assert!(matches!(parse("1.5", &EURO), Parsed::Invalid(why) if why.contains("komma als decimaalteken")));
        assert!(matches!(parse("12.34,5", &EURO), Parsed::Invalid(_)));
        // With a decimal point the roles swap.
        let english = NumberFormat::new(2).point();
        assert_eq!(number("1,234.56", &english), Some(1234.56));
        assert!(matches!(parse("1,5", &english), Parsed::Invalid(_)));
    }

    #[test]
    fn decimals_limits_units_and_signs_follow_the_format() {
        assert!(matches!(parse("2,345", &EURO), Parsed::Invalid(why) if why == "Gebruik ten hoogste 2 decimalen."), "no silent rounding");
        assert!(matches!(parse("3,5", &COUNT), Parsed::Invalid(why) if why == "Alleen hele getallen."));
        assert!(matches!(parse("-1", &COUNT), Parsed::Invalid(why) if why == "Minimaal 0."));
        assert_eq!(number("−12,5", &EURO), Some(-12.5), "a real minus sign also counts");
        let area = NumberFormat::new(1).unit("m²");
        assert_eq!(number("164,0 m²", &area), Some(164.0));
        assert_eq!(number(",5", &EURO), Some(0.5));
        assert_eq!(parse("", &EURO), Parsed::Empty);
        assert_eq!(parse("-", &EURO), Parsed::Incomplete);
        assert_eq!(parse("1,", &EURO), Parsed::Incomplete);
        assert_eq!(format(1234567.891, &EURO), "1.234.567,89");
        assert_eq!(format(-0.001, &EURO), "0,00");
        assert_eq!(format(1234.5, &NumberFormat::new(1).point()), "1,234.5");
    }

    #[test]
    fn a_draft_keeps_what_was_typed_and_stores_only_valid_numbers() {
        let mut input = NumberInput::new(Value::Number(240.0), &EURO);
        assert_eq!(input.text(), "240,00");
        assert_eq!(input.edit("250,5".into(), &EURO), Some(Value::Number(250.5)));
        assert_eq!(input.edit("250,".into(), &EURO), None, "unfinished: nothing stored");
        assert_eq!(input.error(), None, "and no error while typing");
        assert_eq!(input.edit("25O".into(), &EURO), None, "a letter O is not a zero");
        assert_eq!(input.text(), "25O", "the draft stays");
        assert!(input.error().is_some());
        assert_eq!(input.edit("".into(), &EURO), None);
        assert_eq!(input.error(), Some("Vul een getal in."));
        assert_eq!(input.edit("".into(), &EURO.optional()), Some(Value::Empty), "empty is a value where allowed");
        input.show(Value::Mixed, &EURO);
        assert_eq!((input.text(), input.error()), ("", None));
    }
}
