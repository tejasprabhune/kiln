//! `kiln wave [<test>]`.

use anyhow::{anyhow, Context, Result};

use kiln_core::find_manifest;

pub fn run(test_name: Option<String>, print_path: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();

    let fst = match test_name {
        Some(name) => {
            let p = kiln_wave::fst_path(&project_root, &name);
            if !p.is_file() {
                anyhow::bail!(
                    "no FST wave for test `{name}` at {}. Run `kiln test --trace` first.",
                    p.display()
                );
            }
            p
        }
        None => kiln_wave::most_recent_fst(&project_root)?,
    };

    if print_path {
        println!("{}", fst.display());
        return Ok(());
    }

    kiln_wave::open(&fst)?;
    println!("Opened {} in surfer", fst.display());
    Ok(())
}
