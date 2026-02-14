mod parent {
    use std::collections::HashMap;

    pub struct Foo;

    mod child {
        use super::*;

        fn test() {
            let m: HashMap<String, String> = HashMap::new();
        }
    }
}
fn main() {}
