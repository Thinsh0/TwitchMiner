// Temporary (not committed).
use tm_client::{Backend, Remote};
#[tokio::main]
async fn main() {
    let url = std::env::args().nth(1).unwrap();
    for tok in std::env::args().skip(2) {
        let tok = tok.replace("\\n", "\n").replace("\\u00a0", "\u{a0}");
        match Remote::connect(&url, &tok).await {
            Ok(r) => println!("{tok:?} => link {:?}", *r.link().borrow()),
            Err(e) => println!("{tok:?} => err {} {}", e.code, e.message),
        }
    }
}
