extern crate cabot;

use cabot::{Client, RequestBuilder};

fn main() {
    let request = RequestBuilder::new("https://www.rust-lang.org/")
        .build()
        .unwrap();
    let client = Client::new();
    let response = client.execute(&request).unwrap();
    print!("{}", response.body_as_string().unwrap());
}