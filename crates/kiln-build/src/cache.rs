//! Content-hashed build cache rooted at `target/kiln/<hash>/`.

use std::path::{Path, PathBuf};

use blake3::Hasher;

use crate::plan::BuildPlan;

/// 32-character lowercase hex hash that uniquely keys a build.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BuildCacheKey(String);

impl BuildCacheKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Compute the cache key for a plan. Includes file *content* hashes
    /// (not just paths or mtimes), the top module, defines, include
    /// directories, profile, and a fixed schema version. Changing any of
    /// these invalidates the cache deterministically; bumping
    /// [`SCHEMA_VERSION`] invalidates *every* cached build (use sparingly).
    pub fn for_plan(plan: &BuildPlan) -> std::io::Result<Self> {
        let mut hasher = Hasher::new();
        hasher.update(SCHEMA_VERSION.as_bytes());
        hasher.update(plan.top.as_bytes());
        hasher.update(plan.profile.as_str().as_bytes());
        hasher.update(if plan.trace { b"trace=1" } else { b"trace=0" });
        if let Some(ts) = &plan.timescale {
            hasher.update(b"timescale=");
            hasher.update(ts.as_bytes());
            hasher.update(b"\0");
        }
        if let Some(lang) = &plan.language {
            hasher.update(b"language=");
            hasher.update(lang.as_bytes());
            hasher.update(b"\0");
        }
        for lib in &plan.libraries {
            hasher.update(b"lib=");
            hasher.update(lib.to_string_lossy().as_bytes());
            hasher.update(b"\0");
        }
        for flag in &plan.verilator_lint_flags {
            hasher.update(flag.as_bytes());
            hasher.update(b"\0");
        }

        let mut sorted_defines: Vec<_> = plan.defines.iter().collect();
        sorted_defines.sort();
        for (k, v) in sorted_defines {
            hasher.update(k.as_bytes());
            hasher.update(b"=");
            hasher.update(v.as_bytes());
            hasher.update(b"\0");
        }
        for inc in &plan.include_dirs {
            hasher.update(inc.to_string_lossy().as_bytes());
            hasher.update(b"\0");
        }
        let mut sorted_sources: Vec<_> = plan.sources.iter().collect();
        sorted_sources.sort();
        for src in sorted_sources {
            // Hash the path *and* the content. Two distinct files with the
            // same content should still produce different keys if their
            // paths differ (because Verilator records source file names in
            // its output).
            hasher.update(src.to_string_lossy().as_bytes());
            hasher.update(b"\0");
            let bytes = std::fs::read(src)?;
            hasher.update(&bytes);
        }
        let hex = hasher.finalize().to_hex();
        Ok(BuildCacheKey(hex.as_str()[..32].to_string()))
    }
}

/// Bump this when the cache layout or invocation flags change in a way
/// that should invalidate every existing cached build.
pub const SCHEMA_VERSION: &str = "kiln-build-cache-v1";

/// Resolve the on-disk directory for a key. The directory may not exist
/// yet; backends are responsible for creating it before writing.
pub fn cache_dir(project_root: &Path, key: &BuildCacheKey) -> PathBuf {
    project_root.join("target").join("kiln").join(key.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Profile;
    use std::collections::BTreeMap;

    fn write_src(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    fn plan_for(sources: Vec<PathBuf>, profile: Profile) -> BuildPlan {
        BuildPlan {
            project_root: PathBuf::from("/proj"),
            top: "top".to_string(),
            sources,
            include_dirs: vec![],
            defines: BTreeMap::new(),
            profile,
            trace: false,
            timescale: None,
            language: None,
            libraries: vec![],
            verilator_lint_flags: vec![],
            extra_verilator_args: vec![],
        }
    }

    #[test]
    fn identical_inputs_yield_same_key() {
        let tmp = tempfile::tempdir().unwrap();
        let s = write_src(tmp.path(), "a.sv", "module a; endmodule");
        let plan = plan_for(vec![s.clone()], Profile::Debug);
        let k1 = BuildCacheKey::for_plan(&plan).unwrap();
        let k2 = BuildCacheKey::for_plan(&plan).unwrap();
        assert_eq!(k1, k2);
        assert_eq!(k1.as_str().len(), 32);
    }

    #[test]
    fn editing_source_changes_key() {
        let tmp = tempfile::tempdir().unwrap();
        let s = write_src(tmp.path(), "a.sv", "module a; endmodule");
        let plan = plan_for(vec![s.clone()], Profile::Debug);
        let before = BuildCacheKey::for_plan(&plan).unwrap();
        std::fs::write(&s, "module a;\n  // a comment\nendmodule").unwrap();
        let after = BuildCacheKey::for_plan(&plan).unwrap();
        assert_ne!(before, after);
    }

    #[test]
    fn changing_profile_changes_key() {
        let tmp = tempfile::tempdir().unwrap();
        let s = write_src(tmp.path(), "a.sv", "module a; endmodule");
        let debug = BuildCacheKey::for_plan(&plan_for(vec![s.clone()], Profile::Debug)).unwrap();
        let release = BuildCacheKey::for_plan(&plan_for(vec![s], Profile::Release)).unwrap();
        assert_ne!(debug, release);
    }

    #[test]
    fn changing_define_changes_key() {
        let tmp = tempfile::tempdir().unwrap();
        let s = write_src(tmp.path(), "a.sv", "module a; endmodule");
        let mut p1 = plan_for(vec![s.clone()], Profile::Debug);
        let mut p2 = p1.clone();
        p1.defines.insert("X".into(), "1".into());
        p2.defines.insert("X".into(), "2".into());
        assert_ne!(
            BuildCacheKey::for_plan(&p1).unwrap(),
            BuildCacheKey::for_plan(&p2).unwrap()
        );
    }

    #[test]
    fn cache_dir_layout() {
        let key = BuildCacheKey("abc123".to_string());
        let dir = cache_dir(Path::new("/proj"), &key);
        assert_eq!(dir, PathBuf::from("/proj/target/kiln/abc123"));
    }
}
