pub struct User {
    pub name: String,
    pub email: String,
}

impl User {
    pub fn new(name: &str, email: &str) -> User {
        User {
            name: name.into(),
            email: email.into(),
        }
    }

    pub fn greeting(&self) -> String {
        format!("Hello, {}!", self.name)
    }

    pub fn display_line(&self) -> String {
        format!("{} <{}>", self.name, self.email)
    }
}
