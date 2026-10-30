fn main() {
    let bytes = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    println!("{}", alloy_core::content::scan::cf_fingerprint(&bytes));
}
