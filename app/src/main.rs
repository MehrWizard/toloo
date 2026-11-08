#[cfg(not(target_os = "android"))]
fn main() -> anyhow::Result<()> {
    toloo_app::run(toloo_app::default_data_dir())
}

#[cfg(target_os = "android")]
fn main() {}
