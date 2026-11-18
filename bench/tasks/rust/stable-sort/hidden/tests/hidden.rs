use people::{sort_people, Person};

fn person(name: &str, age: u32) -> Person {
    Person { name: name.into(), age }
}

#[test]
fn oldest_first_and_ties_keep_their_order() {
    let mut list = vec![person("a", 30), person("b", 40), person("c", 30), person("d", 50), person("e", 40)];
    sort_people(&mut list);
    let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["d", "b", "e", "a", "c"]);
}

#[test]
fn many_ties_keep_their_order() {
    let mut list: Vec<Person> = (0..300).map(|i| person(&format!("p{i}"), (i % 3) as u32)).collect();
    sort_people(&mut list);
    for age in 0..3 {
        let order: Vec<usize> = list
            .iter()
            .filter(|p| p.age == age)
            .map(|p| p.name[1..].parse().unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "age {age}: {order:?}");
    }
    assert_eq!(list[0].age, 2);
}
