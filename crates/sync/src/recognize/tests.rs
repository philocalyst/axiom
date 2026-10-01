use super::*;

#[test]
fn a_path_is_known_by_its_full_name_and_each_path_suffix() {
    let known = aliases("paypal/john/credit");
    assert_eq!(known, ["credit", "john/credit", "paypal/john/credit"]);
}

#[test]
fn typed_captures_keep_original_amount_separate_from_statement_amount() {
    assert_eq!(BUILTINS[2], Capture::Amount);
    assert_eq!(BUILTINS[3], Capture::Original);
}
