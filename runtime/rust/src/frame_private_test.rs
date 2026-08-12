use super::solum_home_value;

#[test]
fn solum_home_value_prefers_home() {
    assert_eq!(
        solum_home_value(Some("/home/faber".into()), Some("C:\\Users\\faber".into())),
        Ok("/home/faber".into())
    );
}

#[test]
fn solum_home_value_falls_back_to_userprofile() {
    assert_eq!(
        solum_home_value(None, Some("C:\\Users\\faber".into())),
        Ok("C:\\Users\\faber".into())
    );
}

#[test]
fn solum_home_value_errors_without_either_environment_variable() {
    assert_eq!(
        solum_home_value(None, None),
        Err("no home directory environment variable")
    );
}

#[test]
fn solum_home_value_accepts_empty_home() {
    assert_eq!(
        solum_home_value(Some("".into()), Some("C:\\Users\\faber".into())),
        Ok("".into())
    );
}

#[test]
fn solum_home_value_empty_home_ignores_userprofile() {
    assert_eq!(solum_home_value(Some("".into()), None), Ok("".into()));
}
