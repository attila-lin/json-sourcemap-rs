# json-sourcemap.rs

Just a [json-source-map](https://github.com/epoberezkin/json-source-map)'s port to Rust.

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
json-sourcemap = "0.2"
```

## Example

```rust
use json_sourcemap::Options;

// fn main() {
    let json = r#"{
        "foo": "bar",
        "baz": 42
    }"#;

    let options = Options::default();
    let map = json_sourcemap::parse(json, options).unwrap();

    println!("{:?}", map);

    println!("{:?}", map.get_location("/foo").unwrap());

    let locs = map.get_location("/baz").unwrap();
    println!("{:?}", locs.key())
// }
```
