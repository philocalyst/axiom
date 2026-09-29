use axiom_v2::{Date, Number, Value};
use std::str::FromStr;

#[test]
fn exact_numbers_have_one_semantic_spelling() {
    let left = Number::from_str("15.00").unwrap();
    let right = Number::from_str("30/2").unwrap();
    assert_eq!(left, right);
    assert_eq!(left.to_string(), "15");
    assert_eq!(serde_json::to_string(&left).unwrap(), r#""15""#);
    assert!(serde_json::from_str::<Number>(r#""15.0""#).is_err());
    let half = left.div(&Number::from_str("6").unwrap()).unwrap();
    assert_eq!(half.to_string(), "2.5");
    assert_eq!(serde_json::to_string(&half).unwrap(), r#""5/2""#);
}

#[test]
fn dates_are_orderable_and_calendar_validated() {
    let earlier = Date::from_str("2024-02-29").unwrap();
    let later = Date::from_str("2024-03-01").unwrap();
    assert!(earlier < later);
    assert_eq!(serde_json::to_string(&earlier).unwrap(), r#""2024-02-29""#);
    assert!(Date::from_str("2023-02-29").is_err());
    assert!(Date::from_str("0000-01-01").is_err());
}

#[test]
fn values_parse_and_display_nested_typed_literals() {
    let value = Value::parse(
        r#"{amount: 1/3 USD, booked: 2026-09-26, label: "cash #1", tags: [true, @sale/one, ?lot]}"#,
    )
    .unwrap();
    let text = value.to_string();
    assert_eq!(Value::parse(&text).unwrap(), value);
    assert!(Value::parse("[4, 5]").is_ok());
    assert!(Value::parse("2 USD").is_ok());
    assert!(Value::parse("@broken name").is_err());
    assert!(Value::parse("2026-13-01").is_err());
}

#[test]
fn value_parser_rejects_excessive_nesting_and_validate_guards_decoded_values() {
    let nested = format!("{}0{}", "[".repeat(70), "]".repeat(70));
    assert!(Value::parse(&nested).is_err());

    // Serde decoding is a data codec, not semantic authorization. The model
    // validation boundary rejects directly decoded invalid variants.
    let decoded: Value = serde_json::from_str(r#"{"Quantity":["1","bad unit"]}"#).unwrap();
    assert!(decoded.validate().is_err());
}
