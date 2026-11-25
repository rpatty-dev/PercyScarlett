fn main() {
    println!("cargo:rustc-link-arg-cdylib=/DELAYLOAD:percyport-probe-delayed.dll");
    println!("cargo:rustc-link-lib=delayimp");
}
