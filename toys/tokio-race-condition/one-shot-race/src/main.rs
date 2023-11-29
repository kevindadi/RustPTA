#[tokio::main]
async fn main() {
    loop {
        // let (tx, mut rx) = tokio::sync::oneshot::channel();
        // tokio::spawn(async move {
        //     let _ = tx.send(());
        // });
        // tokio::spawn(async move {
        //     rx.close();
        //     let _ = rx.try_recv();
        // });
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            if let Err(mut s) = tx.send(String::from("hello")) {
                s.push_str(" world");
                assert_eq!(&s[..], "hello world");
            }
        });
        tokio::spawn(async move {
            rx.close();
            if let Ok(mut s) = rx.try_recv() {
                s.push_str(" san francisco");
                assert_eq!(&s[..], "hello san francisco");
            }
        });
    }
}

#[test]
#[should_panic]
fn one_shot_race() {
    loom::model(|| loop {
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            if let Err(mut s) = tx.send(String::from("hello")) {
                s.push_str(" world");
                assert_eq!(&s[..], "hello world");
            }
        });
        tokio::spawn(async move {
            rx.close();
            if let Ok(mut s) = rx.try_recv() {
                s.push_str(" san francisco");
                assert_eq!(&s[..], "hello san francisco");
            }
        });
    });
}
