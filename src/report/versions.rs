//! Stable patch compatibility is policy; recorded native tests are separate evidence.
#[derive(Clone, Copy)]
pub struct Policy {
    pub minimum: [u32; 3],
    pub tested: &'static [&'static str],
}

pub const CLAUDE: Policy = Policy {
    minimum: [2, 1, 267],
    tested: &["2.1.267", "2.1.268"],
};
pub const CODEX: Policy = Policy {
    minimum: [0, 153, 0],
    tested: &["0.153.0"],
};
pub const PI: Policy = Policy {
    minimum: [0, 85, 1],
    tested: &["0.85.1"],
};
pub const OMP: Policy = Policy {
    minimum: [18, 2, 2],
    tested: &["18.1.19"],
};

/// Only canonical stable x.y.z releases, not prereleases, build metadata or leading zeros.
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
    pub fn accepts(self, version: &str) -> bool {
        parse(version).is_some_and(|v| v >= self.minimum)
    }
    pub fn range(self) -> String {
        let [major, minor, patch] = self.minimum;
        format!(">={major}.{minor}.{patch} (stable only)")
    }
    pub fn tested(self, version: Option<&str>) -> bool {
        version.is_some_and(|v| self.tested.contains(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compatibility_requires_stable_release_at_or_above_floor() {
        for (policy, good, bad) in [
            (CLAUDE, "2.1.274", "2.1.266"),
            (CODEX, "0.153.9", "0.152.9"),
            (PI, "0.85.9", "0.85.0"),
            (OMP, "18.2.9", "18.2.1"),
        ] {
            assert!(policy.accepts(good));
            assert!(!policy.accepts(bad));
            let [a, b, c] = policy.minimum;
            assert!(policy.accepts(&format!("{a}.{b}.{c}")));
            assert!(policy.accepts(&format!("{a}.{}.0", b + 1)));
            assert!(policy.accepts(&format!("{}.0.0", a + 1)));
            for version in [
                format!("{good}-beta"),
                format!("{good}+local"),
                format!("0{good}"),
                "1.2".into(),
                "1.2.3.4".into(),
                "1.2.4294967296".into(),
            ] {
                assert!(!policy.accepts(&version), "{version}");
            }
        }
    }
}
