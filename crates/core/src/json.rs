//! Lecture prudente de JSON non typé (portage de `src/narrow.ts`) : un fichier écrit à la main est
//! validé champ par champ, avec un message par champ fautif.

use serde_json::{Map, Value};

use crate::geo::LonLat;

pub type Object = Map<String, Value>;

pub fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

pub fn non_empty(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.trim().is_empty())
}

pub fn lon_lat(value: &Value) -> Option<LonLat> {
    match value.as_array()?.as_slice() {
        [lon, lat] => Some([finite(lon)?, finite(lat)?]),
        _ => None,
    }
}

/// Champ `key` d'un objet, `None` si `value` n'en est pas un ou si le champ manque.
pub fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_object()?.get(key)
}

/// JSON compact, écrit comme `JSON.stringify` (entiers sans `.0`, `0.000007` plutôt que `7e-6`) : un
/// fichier recalculé à l'identique ne change pas d'un octet, quel que soit l'outil qui l'a écrit.
pub fn to_js_string(value: &impl serde::Serialize) -> String {
    let mut out = String::new();
    write_js(&serde_json::to_value(value).expect("valeur sérialisable"), &mut out);
    out
}

fn write_js(value: &Value, out: &mut String) {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(float) if number.is_f64() => out.push_str(&js_number(float)),
            _ => out.push_str(&number.to_string()),
        },
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_js(item, out);
            }
            out.push(']');
        }
        Value::Object(fields) => {
            out.push('{');
            for (i, (key, item)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push(':');
                write_js(item, out);
            }
            out.push('}');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// `Number.prototype.toString` : les chiffres les plus courts (ceux de serde_json, départagés comme en
/// JavaScript), en notation fixe de 1e-6 à 1e21.
fn js_number(float: f64) -> String {
    if float == 0.0 {
        return "0".into();
    }
    let text = serde_json::to_string(&float.abs()).expect("nombre fini");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let exponent: i32 = exponent.parse().expect("exposant");
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all = format!("{whole}{fraction}");
    let digits = all.trim_start_matches('0');
    // Valeur = 0.<digits> × 10^n.
    let n = exponent + whole.len() as i32 - (all.len() - digits.len()) as i32;
    let digits = digits.trim_end_matches('0');
    let k = digits.len() as i32;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let head = if k == 1 { digits.to_owned() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        format!("{head}e{sign}{}", e.abs())
    };
    if float < 0.0 { format!("-{body}") } else { body }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn writes_integral_floats_like_javascript() {
        assert_eq!(to_js_string(&json!({ "a": [0.0, -0.0, 2.5, 1000.0], "b": 3 })), r#"{"a":[0,0,2.5,1000],"b":3}"#);
        assert_eq!(
            to_js_string(&json!([-0.000007, 7e-7, 1.5e-8, 1e21, 0.1 + 0.2])),
            "[-0.000007,7e-7,1.5e-8,1e+21,0.30000000000000004]"
        );
        assert_eq!(
            to_js_string(&json!([18_863_845.461_914_062, 123_456_789_012_345_680_000.0, 1e16, 5e-324, 1234.5])),
            "[18863845.461914062,123456789012345680000,10000000000000000,5e-324,1234.5]"
        );
        assert_eq!(to_js_string(&json!({ "nom": "Saül \"x\"" })), r#"{"nom":"Saül \"x\""}"#);
    }
}
