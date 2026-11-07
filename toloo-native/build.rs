fn main() {
    // The chat UI uses array lookup helpers for multi-select state.
    unsafe { std::env::set_var("SLINT_ENABLE_EXPERIMENTAL_FEATURES", "1") };
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");
}
