use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Debug)]
struct Bag {
    attributes: Arc<Mutex<Vec<usize>>>,
}

impl Bag {
    fn new(n: usize) -> Self {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(0);
        }

        Bag {
            attributes: Arc::new(Mutex::new(v)),
        }
    }

    fn item_action(&self, item_attr1: usize, item_attr2: usize) -> Result<(), ()> {
        // let attributes = self.attributes.lock().unwrap();
        // if attributes.contains(&item_attr1) || attributes.contains(&item_attr2) {
        //     println!(
        //         "Item attributes {} and {} are in Bag attribute list!",
        //         item_attr1, item_attr2
        //     );
        //     Ok(())
        // } else {
        //     Err(())
        // }
        // let condition = {
        //     let lock1 = self.attributes.lock().unwrap();
        //     let lock2 = self.attributes.lock().unwrap();
        //     lock1.contains(&item_attr1) || lock2.contains(&item_attr2)
        // };
        // if condition {
        //     println!(
        //         "Item attributes {} and {} are in Bag attribute list!",
        //         item_attr1, item_attr2
        //     );
        //     Ok(())
        // } else {
        //     Err(())
        // }
        if self.attributes.lock().unwrap().contains(&item_attr1)
            || self.attributes.lock().unwrap().contains(&item_attr2)
        {
            println!(
                "Item attributes {} and {} are in Bag attribute list!",
                item_attr1, item_attr2
            );
            Ok(())
        } else {
            Err(())
        }
    }
}

#[derive(Clone, Debug)]
struct Item {
    item_attr1: usize,
    item_attr2: usize,
}

impl Item {
    pub fn new(item_attr1: usize, item_attr2: usize) -> Self {
        Item {
            item_attr1,
            item_attr2,
        }
    }
}

fn main() {
    let mut item_list: Vec<Item> = Vec::new();
    for i in 0..10 {
        item_list.push(Item::new(i, (i + 1) % 10));
    }

    let bag: Bag = Bag::new(10); //create 10 attributes

    let mut handles = Vec::with_capacity(10);

    for x in 0..10 {
        let bag2 = bag.clone();
        let item_list2 = item_list.clone();

        handles.push(thread::spawn(move || {
            bag2.item_action(item_list2[x].item_attr1, item_list2[x].item_attr2);
        }))
    }

    for h in handles {
        println!("Here");
        h.join().unwrap();
    }
}

#[test]
fn one_shot_race() {
    loom::model(|| {
        let mut item_list: Vec<Item> = Vec::new();
        for i in 0..10 {
            item_list.push(Item::new(i, (i + 1) % 10));
        }

        let bag: Bag = Bag::new(10); //create 10 attributes

        let mut handles = Vec::with_capacity(10);

        for x in 0..10 {
            let bag2 = bag.clone();
            let item_list2 = item_list.clone();

            handles.push(thread::spawn(move || {
                bag2.item_action(item_list2[x].item_attr1, item_list2[x].item_attr2);
            }))
        }

        for h in handles {
            println!("Here");
            h.join().unwrap();
        }
    });
}
