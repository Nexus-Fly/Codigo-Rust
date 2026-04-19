use tashi_vertex::KeySecret;

fn main() {
    let secret = KeySecret::generate();
    let public  = secret.public();
    println!("Secret: {secret}");
    println!("Public: {public}");
}
