use super::*;

#[test]
fn test_lunar_new_year_and_mid_autumn() {
    for (date, expected) in [
        ("1900-01-31", (1900, 1, false, 1)),
        ("2024-02-10", (2024, 1, false, 1)),
        ("2024-09-17", (2024, 8, false, 15)),
        ("2023-03-22", (2023, 2, true, 1)),
    ] {
        let date: NaiveDate = date.parse().unwrap();
        assert_eq!(from_solar(date), Some(expected));
        assert_eq!(anniversary(expected.0, expected.1, expected.2, expected.3), Some(date));
    }
}

#[test]
fn test_anniversary_missing_leap_month_and_short_month() {
    assert_eq!(anniversary(2024, 2, true, 1), anniversary(2024, 2, false, 1));
    assert_eq!(anniversary(2024, 1, false, 30), anniversary(2024, 1, false, 29));
    let today = "2024-09-18".parse().unwrap();
    assert_eq!(next_annual(today, true, 8, false, 15), Some("2025-10-06".parse().unwrap()));
    assert!(anniversary(2100, 12, false, 30).is_some());
    assert!(anniversary(2101, 1, false, 1).is_none());
}
