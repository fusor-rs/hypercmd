fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "terminal")]
    hypercmd_build::compile_app()?;
    #[cfg(feature = "browser")]
    fusor_build::compile_app()?;
    Ok(())
}
