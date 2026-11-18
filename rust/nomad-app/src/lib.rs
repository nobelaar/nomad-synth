//! Console front end of the Rust NOMAD: the demo scene and argument parsing.

pub mod scene;

/// `--name value` flags after the subcommand (no CLI dependency).
pub struct Args {
    pairs: Vec<(String, String)>,
}

impl Args {
    pub fn parse(rest: &[String]) -> Result<Args, String> {
        let mut pairs = Vec::new();
        let mut it = rest.iter();
        while let Some(flag) = it.next() {
            let Some(name) = flag.strip_prefix("--") else {
                return Err(format!("unexpected argument: {flag}"));
            };
            let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
            pairs.push((name.to_string(), value.clone()));
        }
        Ok(Args { pairs })
    }

    /// Value of `--name` parsed as `T`, `default` if absent; error if malformed or unknown flag.
    pub fn get<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.pairs.iter().rev().find(|(n, _)| n == name) {
            Some((_, v)) => v
                .parse()
                .map_err(|_| format!("bad value for --{name}: {v}")),
            None => Ok(default),
        }
    }

    pub fn opt<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, String> {
        match self.pairs.iter().rev().find(|(n, _)| n == name) {
            Some((_, v)) => v
                .parse()
                .map(Some)
                .map_err(|_| format!("bad value for --{name}: {v}")),
            None => Ok(None),
        }
    }

    /// Fail on flags the subcommand does not know.
    pub fn only(&self, known: &[&str]) -> Result<(), String> {
        match self
            .pairs
            .iter()
            .find(|(n, _)| !known.contains(&n.as_str()))
        {
            Some((n, _)) => Err(format!("unknown flag --{n}")),
            None => Ok(()),
        }
    }
}
pub mod bench;
