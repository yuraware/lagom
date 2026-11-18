#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub name: String,
    pub age: u32,
}

/// Oldest first; people of the same age keep the order they were given in.
pub fn sort_people(people: &mut [Person]) {
    people.sort_unstable_by(|a, b| a.age.cmp(&b.age));
}
