//! Recorded native evidence only. OVRCR never gates a harness on its version: launch
//! admission, reporting and quota all decide support from argv, hooks and the native
//! protocol itself. Doctors print the observed version beside the releases that have
//! recorded native tests, so a new release is evidence-free, never refused.
#[derive(Clone, Copy)]
pub struct Policy {
    pub tested: &'static [&'static str],
}

pub const CLAUDE: Policy = Policy {
    tested: &["2.1.267", "2.1.268"],
};
pub const CODEX: Policy = Policy {
    tested: &["0.153.0"],
};
pub const PI: Policy = Policy {
    tested: &["0.85.1"],
};
pub const OMP: Policy = Policy {
    tested: &["18.1.19"],
};
pub const GROK: Policy = Policy {
    tested: &["1.0.40"],
};

/// Only canonical stable x.y.z releases, not prereleases, build metadata or leading zeros.
/// Used to report a version, never to admit or refuse one.
pub fn parse(version: &str) -> Option<[u32; 3]> {
    let mut parts = version.split('.');
    let mut values = [0; 3];
    for value in &mut values {
        let part = parts.next()?;
        if part.is_empty()
            || (part.len() > 1 && part.starts_with('0'))
            || !part.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        *value = part.parse().ok()?;
    }
    parts.next().is_none().then_some(values)
}

impl Policy {
    pub fn tested(self, version: Option<&str>) -> bool {
        version.is_some_and(|v| self.tested.contains(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_are_parsed_for_reporting_and_tested_is_exact_evidence() {
        assert_eq!(parse("0.161.0"), Some([0, 161, 0]));
        assert_eq!(parse("2.1.293"), Some([2, 1, 293]));
        for version in [
            "0.161.0-beta",
            "0.161.0+local",
            "00.1.0",
            "1.2",
            "1.2.3.4",
            "1.2.4294967296",
        ] {
            assert_eq!(parse(version), None, "{version}");
        }
        assert!(CODEX.tested(Some("0.153.0")));
        // Untested is not unsupported: there is no floor and no ceiling.
        assert!(!CODEX.tested(Some("0.161.0")));
        assert!(!CODEX.tested(None));
    }
}
