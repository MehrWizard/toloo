#[cfg(not(target_os = "android"))]
fn main() -> anyhow::Result<()> {
    toloo_native::run(toloo_native::default_data_dir())
}

#[cfg(target_os = "android")]
fn main() {}
