use crate::graph::text;
use ifc_lite_core::AttributeValue as A;
use openbim_ids::Value;

pub fn supported(v: &Value) -> Result<(), String> {
    if let Value::Restriction(r) = v {
        let ordered = [
            &r.min_inclusive,
            &r.max_inclusive,
            &r.min_exclusive,
            &r.max_exclusive,
        ];
        for bound in ordered.into_iter().flatten() {
            if !matches!(r.base.as_str(), "integer" | "decimal" | "double" | "float") {
                return Err("Ordered restrictions on nonnumeric values are unsupported".into());
            }
            if !bound.parse::<f64>().is_ok_and(|v| v.is_finite()) {
                return Err("Invalid numeric restriction bound".into());
            }
        }
        for pattern in &r.patterns {
            pattern_match(pattern, "")?;
        }
        if r.total_digits.is_some() || r.fraction_digits.is_some() {
            return Err("XSD digit restrictions are not yet supported".into());
        }
        if !matches!(
            r.base.as_str(),
            "string" | "normalizedString" | "integer" | "decimal" | "double" | "float" | "boolean"
        ) {
            return Err(format!("Unsupported XSD base: {}", r.base));
        }
    }
    Ok(())
}
pub fn matches(v: &Value, s: &str) -> Result<bool, String> {
    supported(v)?;
    match v {
        Value::Simple(expected) => Ok(s == expected),
        Value::Restriction(r) => {
            if !r.patterns.is_empty() {
                let mut ok = false;
                for pattern in &r.patterns {
                    ok |= pattern_match(pattern, s)?;
                }
                if !ok {
                    return Ok(false);
                }
            }
            let numeric = matches!(r.base.as_str(), "integer" | "decimal" | "double" | "float");
            let n = if numeric {
                match s.trim().parse::<f64>() {
                    Ok(v) if v.is_finite() => Some(v),
                    _ => return Ok(false),
                }
            } else {
                None
            };
            if r.base == "integer" && n.is_some_and(|v| v.abs() > 9_007_199_254_740_992.) {
                return Err("Integer restriction exceeds exact numeric evaluation range".into());
            }
            if r.base == "integer" && n.is_some_and(|v| v.fract() != 0.) {
                return Ok(false);
            }
            if r.base == "boolean" && !matches!(s, "true" | "false" | "1" | "0") {
                return Ok(false);
            }
            if !r.enumeration.is_empty()
                && !r.enumeration.iter().any(|e| {
                    if let Some(n) = n {
                        e.parse::<f64>().is_ok_and(|e| {
                            if r.base == "integer" {
                                n == e
                            } else {
                                nearly_equal(n, e)
                            }
                        })
                    } else {
                        s == e
                    }
                })
            {
                return Ok(false);
            }
            let len = s.chars().count() as u64;
            if r.length.is_some_and(|l| len != l)
                || r.min_length.is_some_and(|l| len < l)
                || r.max_length.is_some_and(|l| len > l)
            {
                return Ok(false);
            }
            for (bound, mode) in [
                (&r.min_inclusive, 0),
                (&r.max_inclusive, 1),
                (&r.min_exclusive, 2),
                (&r.max_exclusive, 3),
            ] {
                if let Some(bound) = bound {
                    let n = n.ok_or("Ordered restrictions on nonnumeric values are unsupported")?;
                    let b = bound
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite())
                        .ok_or("Invalid numeric restriction bound")?;
                    if match mode {
                        0 => n < b,
                        1 => n > b,
                        2 => n <= b,
                        _ => n >= b,
                    } {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        }
    }
}
pub fn value_matches(expected: &Option<Value>, a: &A) -> Result<bool, String> {
    match a {
        A::Null => Ok(false),
        A::Enum(s) if s == "U" => Ok(false),
        A::Enum(s) if s == "T" || s == "F" => {
            let value = if s == "T" { "true" } else { "false" };
            expected.as_ref().map_or(Ok(true), |v| matches(v, value))
        }
        A::Derived => Err("Derived IFC attributes require evaluation".into()),
        A::List(values) => {
            let mut ok = false;
            for v in values {
                ok |= value_matches(expected, v)?;
            }
            Ok(ok)
        }
        A::EntityRef(_) | A::Binary(_) => {
            Err("Reference-valued and binary attribute comparison is unsupported".into())
        }
        A::Typed { name, value } => {
            if name.as_ref() == "IFCBOOLEAN" {
                let s = match text(value).as_deref() {
                    Some("T") => "true",
                    Some("F") => "false",
                    _ => return Ok(false),
                };
                return expected.as_ref().map_or(Ok(true), |e| matches(e, s));
            }
            value_matches(expected, value)
        }
        _ => {
            let s = text(a).ok_or("Unsupported IFC scalar value")?;
            if s.is_empty() {
                return Ok(false);
            }
            expected.as_ref().map_or(Ok(true), |v| {
                if let Value::Simple(e) = v {
                    if let A::Integer(actual) = a {
                        return Ok(e.parse::<i64>().is_ok_and(|expected| *actual == expected));
                    }
                    if matches!(a, A::Float(_)) {
                        return Ok(match (s.parse::<f64>(), e.parse::<f64>()) {
                            (Ok(a), Ok(b)) => nearly_equal(a, b),
                            _ => false,
                        });
                    }
                }
                matches(v, &s)
            })
        }
    }
}

thread_local! { static PATTERNS: std::cell::RefCell<std::collections::HashMap<String,oxixml_regex::Regex>> = std::cell::RefCell::new(std::collections::HashMap::new()); }
fn pattern_match(pattern: &str, s: &str) -> Result<bool, String> {
    if pattern.len() > 4096 || s.len() > 1024 * 1024 {
        return Err("XSD pattern/input exceeds evaluation limit".into());
    }
    PATTERNS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(pattern) {
            if cache.len() >= 128 {
                cache.clear();
            }
            let regex = oxixml_regex::Regex::xsd_versioned(pattern, oxixml_regex::XsdVersion::V1_0)
                .map_err(|e| format!("Invalid XSD pattern: {e}"))?;
            cache.insert(pattern.to_string(), regex);
        }
        cache[pattern]
            .try_is_match(s)
            .map_err(|e| format!("XSD pattern evaluation: {e}"))
    })
}

pub fn nearly_equal(a: f64, b: f64) -> bool {
    a >= (b - b.abs() * 1e-6 - 1e-6).next_down() && a <= (b + b.abs() * 1e-6 + 1e-6).next_up()
}
