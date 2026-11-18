use users::User;

#[test]
fn the_field_is_renamed_and_nothing_else_changed() {
    let user = User {
        display_name: "Ann".into(),
        email: "ann@example.com".into(),
    };
    assert_eq!(user.greeting(), "Hello, Ann!");
    assert_eq!(user.display_line(), "Ann <ann@example.com>");
    assert_eq!(User::new("Bo", "bo@example.com").display_name, "Bo");
}
