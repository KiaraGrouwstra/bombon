use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::derivation::License;

#[allow(clippy::expect_used)]
static STRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""((?:[^"\\]|\\.)*)""#).expect("Failed to build regex for Ruby strings")
});

/// Read the licenses of a Ruby gem from the gemspec in its store path.
///
/// Gems built from a `gemset.nix` (e.g. by `bundlerEnv`) have no `meta.license`. The gemspec
/// that is installed with the gem, however, contains the licenses the gem declares.
///
/// Returns `None` if the store path doesn't contain exactly one gemspec or if the gemspec
/// declares no licenses.
pub fn licenses(store_path: &Path) -> Option<Vec<License>> {
    let gemspec = fs::read_to_string(find_gemspec(store_path)?).ok()?;
    let licenses = parse_licenses(&gemspec)
        .into_iter()
        .map(|name| convert_license(&name))
        .collect::<Vec<_>>();
    (!licenses.is_empty()).then_some(licenses)
}

/// Find the gemspec of the gem in a store path.
///
/// Gemspecs that are symlinks to another store path are ignored so that an environment (e.g. from
/// `ruby.withPackages`) doesn't get the licenses of the gems inside it.
fn find_gemspec(store_path: &Path) -> Option<PathBuf> {
    let mut gemspecs = fs::read_dir(store_path.join("lib/ruby/gems"))
        .ok()?
        .flatten()
        .filter_map(|entry| fs::read_dir(entry.path().join("specifications")).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| fs::canonicalize(entry.path()).ok())
        .filter(|path| {
            path.starts_with(store_path) && path.extension().is_some_and(|e| e == "gemspec")
        });
    let gemspec = gemspecs.next()?;
    gemspecs.next().is_none().then_some(gemspec)
}

/// Parse the licenses from a gemspec.
///
/// The gemspec has a fixed format, e.g. `s.licenses = ["MIT".freeze]`.
fn parse_licenses(gemspec: &str) -> Vec<String> {
    gemspec
        .lines()
        .find_map(|line| line.trim().strip_prefix("s.licenses = ["))
        .map(|list| {
            STRING
                .captures_iter(list)
                .map(|captures| captures[1].trim().to_string())
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Convert a license name from a gemspec to a license.
///
/// The name is an SPDX identifier if it is one when compared case-insensitively and with spaces
/// replaced by hyphens (e.g. `Apache 2.0` is `Apache-2.0`).
fn convert_license(name: &str) -> License {
    let id = name.replace(' ', "-");
    License {
        full_name: name.to_string(),
        spdx_id: spdx::identifiers::LICENSES
            .iter()
            .find(|license| license.name.eq_ignore_ascii_case(&id))
            .map(|license| license.name.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licenses_from_gemspec() {
        let gemspec = r#"# -*- encoding: utf-8 -*-
# stub: rake 13.3.1 ruby lib

Gem::Specification.new do |s|
  s.name = "rake".freeze
  s.licenses = ["MIT".freeze, "Apache 2.0".freeze, "ruby".freeze, "2-clause BSDL".freeze]
  s.summary = "Rake is a Make-like program implemented in Ruby".freeze
end
"#;
        let licenses = parse_licenses(gemspec)
            .iter()
            .map(|name| convert_license(name))
            .map(|license| (license.full_name, license.spdx_id))
            .collect::<Vec<_>>();
        assert_eq!(
            licenses,
            vec![
                ("MIT".into(), Some("MIT".into())),
                ("Apache 2.0".into(), Some("Apache-2.0".into())),
                ("ruby".into(), Some("Ruby".into())),
                ("2-clause BSDL".into(), None),
            ]
        );
    }

    #[test]
    fn licenses_from_old_gemspec() {
        assert_eq!(
            parse_licenses("  s.licenses = [\"MIT\"]\n"),
            vec!["MIT".to_string()]
        );
    }

    #[test]
    fn no_licenses_in_gemspec() {
        assert!(parse_licenses("  s.name = \"rake\".freeze\n").is_empty());
        assert!(parse_licenses("  s.licenses = []\n").is_empty());
    }
}
