pub struct User {
    pub display_name: String,
    pub email: String,
}

impl User {
    pub fn new(display_name: &str, email: &str) -> User {
        User {
            display_name: display_name.into(),
            email: email.into(),
        }
    }

    pub fn greeting(&self) -> String {
        format!("Hello, {}!", self.display_name)
    }

    pub fn display_line(&self) -> String {
        format!("{} <{}>", self.display_name, self.email)
    }
}
